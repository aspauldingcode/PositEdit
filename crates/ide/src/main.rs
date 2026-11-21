fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--version") {
        println!("posit-edit 0.1.0");
        return;
    }
    if let Some(index) = args.iter().position(|arg| arg == "--detect") {
        let path = args.get(index + 1).map(String::as_str).unwrap_or("");
        println!("{}", ide_tui::detect_path(path));
        return;
    }
    let file = args.into_iter().find(|arg| !arg.starts_with('-'));
    if let Err(error) = ide_tui::run(file.as_deref()) {
        eprintln!("Posit Edit error: {error}");
        std::process::exit(1);
    }
}
