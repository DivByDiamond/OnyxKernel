pub struct Entry {
    pub inode: u32,
    pub data: Vec<u8>,
}

pub struct DirNode {
    pub ino: u32,
    pub parent_ino: u32,
    pub entries: Vec<(String, u32, bool)>,
}

impl DirNode {
    pub fn root() -> Self {
        DirNode {
            ino: 1,
            parent_ino: 1,
            entries: Vec::new(),
        }
    }
}

pub fn add_dir(dirs: &mut Vec<DirNode>, path: &str, next_ino: &mut u32) {
    let parent = find_parent_dir(dirs, path);
    let name = basename(path);
    let ino = *next_ino;
    *next_ino += 1;
    dirs[parent].entries.push((name.to_string(), ino, true));
    dirs.push(DirNode {
        ino,
        parent_ino: dirs[parent].ino,
        entries: Vec::new(),
    });
    eprintln!("  dir {} (ino={})", path, ino);
}

pub fn add_file(
    dirs: &mut Vec<DirNode>,
    files: &mut Vec<Entry>,
    host: &str,
    fs_path: &str,
    next_ino: &mut u32,
) {
    let data = std::fs::read(host).unwrap_or_else(|e| {
        eprintln!("read {}: {}", host, e);
        std::process::exit(1);
    });
    ensure_parent_dirs(dirs, fs_path, next_ino);
    let parent = find_parent_dir(dirs, fs_path);
    let name = basename(fs_path);
    let ino = *next_ino;
    *next_ino += 1;
    dirs[parent].entries.push((name.to_string(), ino, false));
    files.push(Entry { inode: ino, data });
    eprintln!(
        "  {} -> {} (ino={}, {} bytes)",
        host,
        fs_path,
        ino,
        files.last().unwrap().data.len()
    );
}

pub fn add_dir_recursive(
    dirs: &mut Vec<DirNode>,
    files: &mut Vec<Entry>,
    host_dir: &str,
    fs_prefix: &str,
    next_ino: &mut u32,
) {
    let prefix = fs_prefix.trim_end_matches('/').to_string();
    let host_root = std::path::Path::new(host_dir);
    let mut stack = vec![host_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if !dir.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| {
            eprintln!("read_dir {}: {}", dir.display(), e);
            std::process::exit(1);
        }) {
            let entry = entry.unwrap_or_else(|e| {
                eprintln!("read_dir entry: {}", e);
                std::process::exit(1);
            });
            let entry_path = entry.path();
            if let Ok(relative) = entry_path.strip_prefix(host_root) {
                let fs_path = format!("{}/{}", prefix, relative.display());
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    add_dir(dirs, &fs_path, next_ino);
                    stack.push(entry_path);
                } else if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                    add_file(
                        dirs,
                        files,
                        entry_path.to_str().unwrap(),
                        &fs_path,
                        next_ino,
                    );
                }
            }
        }
    }
}

pub fn ensure_parent_dirs(dirs: &mut Vec<DirNode>, path: &str, next_ino: &mut u32) {
    let path = path.trim_start_matches('/');
    let mut cur = String::new();
    for comp in path.split('/') {
        if comp.is_empty() || comp == basename(path) {
            continue;
        }
        cur.push('/');
        cur.push_str(comp);
        let exists = dirs.iter().any(|d| {
            let name = basename(&cur);
            d.parent_ino == dirs[0].ino && d.entries.iter().any(|(n, _, _)| n == name)
        });
        if !exists {
            add_dir(dirs, &cur, next_ino);
        }
    }
}

fn find_parent_dir(dirs: &[DirNode], path: &str) -> usize {
    let path = path.trim_start_matches('/');
    if !path.contains('/') {
        return 0;
    }
    let parent_path = &path[..path.rfind('/').unwrap()];
    let components: Vec<&str> = parent_path.split('/').filter(|s| !s.is_empty()).collect();
    let mut cur_idx = 0;
    for comp in components {
        let mut found = false;
        for d in dirs.iter() {
            if d.parent_ino == dirs[cur_idx].ino {
                for (name, _ino, _is_dir) in &dirs[cur_idx].entries {
                    if name == comp {
                        for (j, dd) in dirs.iter().enumerate() {
                            if dd.ino == *_ino {
                                cur_idx = j;
                                found = true;
                                break;
                            }
                        }
                        if found {
                            break;
                        }
                    }
                }
            }
            if found {
                break;
            }
        }
    }
    cur_idx
}

fn basename(path: &str) -> &str {
    let path = path.trim_start_matches('/');
    match path.rfind('/') {
        Some(i) => &path[i + 1..],
        None => path,
    }
}
