mod compress;
mod convert;

use std::env;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: elf2onx [--ring=1] [--v1] [--compress] <input.elf> <output.onx>");
        process::exit(1);
    }
    let mut ring1 = false;
    let mut v1 = false;
    let mut do_compress = false;
    let mut input = String::new();
    let mut output = String::new();
    for arg in &args[1..] {
        if arg == "--ring=1" {
            ring1 = true;
        } else if arg == "--v1" {
            v1 = true;
        } else if arg == "--compress" {
            do_compress = true;
        } else if input.is_empty() {
            input = arg.clone();
        } else {
            output = arg.clone();
        }
    }
    if input.is_empty() || output.is_empty() {
        eprintln!("usage: elf2onx [--ring=1] [--v1] [--compress] <input.elf> <output.onx>");
        process::exit(1);
    }
    convert::run(&input, &output, ring1, !v1, do_compress);
}
