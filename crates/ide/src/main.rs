fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--version") {
        println!("posit-edit 0.1.0");
        return;
    }
    if args.iter().any(|arg| arg == "--bench-memory") {
        std::process::exit(ide_tui::bench_memory());
    }
    if let Some(index) = args.iter().position(|arg| arg == "--bench") {
        let kind = args
            .get(index + 1)
            .and_then(|value| ide_tui::BenchKind::parse(value))
            .unwrap_or(ide_tui::BenchKind::All);
        if args.get(index + 1).is_some_and(|value| {
            !value.starts_with('-') && ide_tui::BenchKind::parse(value).is_none()
        }) {
            eprintln!(
                "unknown bench {:?}; use all|memory|edit|render|open",
                args[index + 1]
            );
            std::process::exit(2);
        }
        std::process::exit(ide_tui::run_bench(kind));
    }
    if let Some(index) = args.iter().position(|arg| arg == "--detect") {
        let path = args.get(index + 1).map(String::as_str).unwrap_or("");
        println!("{}", ide_tui::detect_path(path));
        return;
    }
    let file = args.into_iter().find(|arg| !arg.starts_with('-'));
    if let Err(error) = ide_tui::run(file.as_deref()) {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            eprintln!("{error}");
        } else {
            eprintln!("Posit Edit error: {error}");
        }
        std::process::exit(1);
    }
}
