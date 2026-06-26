mod generate;
mod fontdata;

use std::fs::File;
use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data = generate::psf1();
    if args.len() > 1 {
        File::create(&args[1])
            .unwrap_or_else(|e| { eprintln!("create {}: {}", args[1], e); std::process::exit(1); })
            .write_all(&data).unwrap();
    } else {
        std::io::stdout().write_all(&data).unwrap();
    }
    eprintln!("psfgen: wrote {} bytes (PSF1, {} glyphs, 8x{})", data.len(), 256, 16);
}
