//! Built-in first-party benchmarks for Posit Edit.
//!
//! Run via `posit-edit --bench` (or `--bench all|memory|edit|render|open`).
//! Stats follow `benchmarks/BUDGETS.md`: ≥5 samples, median + p95.

use ide_core::{
    pressure_level, process_rss_bytes, time_iters, Document, Edit, PressureLevel, SampleStats,
    DEFAULT_SAMPLES, DEFAULT_WARMUP, HISTORY_HARD_MAX, HISTORY_NORMAL, HISTORY_PRESSURE,
};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

use crate::{draw, EditorApp};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchKind {
    All,
    Memory,
    Edit,
    Render,
    Open,
}

impl BenchKind {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "all" => Some(Self::All),
            "memory" | "mem" => Some(Self::Memory),
            "edit" | "edits" | "typing" => Some(Self::Edit),
            "render" | "draw" | "frame" => Some(Self::Render),
            "open" | "load" => Some(Self::Open),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Memory => "memory",
            Self::Edit => "edit",
            Self::Render => "render",
            Self::Open => "open",
        }
    }
}

pub fn run(kind: BenchKind) -> i32 {
    println!("posit-edit bench ({})", kind.label());
    print_host_banner();
    let mut failed = 0;
    let kinds: &[BenchKind] = match kind {
        BenchKind::All => &[
            BenchKind::Memory,
            BenchKind::Edit,
            BenchKind::Render,
            BenchKind::Open,
        ],
        other => &[other],
    };
    for item in kinds {
        let ok = match item {
            BenchKind::All => unreachable!(),
            BenchKind::Memory => bench_memory(),
            BenchKind::Edit => bench_edit(),
            BenchKind::Render => bench_render(),
            BenchKind::Open => bench_open(),
        };
        if !ok {
            failed += 1;
        }
    }
    println!(
        "bench verdict={}",
        if failed == 0 { "PASS" } else { "FAIL" }
    );
    if failed == 0 {
        0
    } else {
        1
    }
}

fn print_host_banner() {
    let level = pressure_level();
    println!("  host_pressure={level:?}");
    if let Some(rss) = process_rss_bytes() {
        println!("  rss_kib={}", rss / 1024);
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = std::process::Command::new("sw_vers")
            .arg("-productVersion")
            .output()
        {
            let ver = String::from_utf8_lossy(&out.stdout);
            println!("  macos={}", ver.trim());
        }
    }
    println!(
        "  samples={DEFAULT_SAMPLES} warmup={DEFAULT_WARMUP} (BUDGETS.md methodology)"
    );
}

fn budget_scale() -> f64 {
    // Debug builds are intentionally slower; gates assume `--release` ×1.
    let debug = if cfg!(debug_assertions) { 5.0 } else { 1.0 };
    let pressure = match pressure_level() {
        PressureLevel::Normal => 1.0,
        PressureLevel::Warning => 1.15,
        PressureLevel::Urgent | PressureLevel::Critical => 1.25,
    };
    debug * pressure
}

fn print_stats(stats: &SampleStats, budget_ms: Option<f64>) -> bool {
    println!("{}", stats.report_line());
    if let Some(budget) = budget_ms {
        let scaled = budget * budget_scale();
        let ok = stats.p95_ms() <= scaled;
        println!(
            "    budget_p95<={scaled:.3}ms{} → {}",
            if cfg!(debug_assertions) {
                " (debug×5)"
            } else {
                ""
            },
            if ok { "PASS" } else { "FAIL" }
        );
        ok
    } else {
        true
    }
}

fn bench_memory() -> bool {
    println!("[memory]");
    let rss_before = process_rss_bytes();
    let payload = "line of sample text for memory pressure\n".repeat(4_000);
    let mut app = EditorApp::new(&payload);
    // Non-coalescing storm (insert at start) stresses history caps.
    for _ in 0..1_500 {
        let buffer = app.focused_mut();
        let rev = buffer.document.revision();
        let _ = buffer.document.apply(
            rev,
            &[Edit {
                start: 0,
                end: 0,
                replacement: "x".into(),
            }],
        );
    }
    let undo_before = app.focused().document.undo_len();
    app.force_memory_pressure(PressureLevel::Critical);
    let undo_after = app.focused().document.undo_len();
    let rss_after = process_rss_bytes();
    println!("  undo_after_storm={undo_before} (cap {HISTORY_NORMAL})");
    println!("  undo_after_critical_trim={undo_after} (cap {HISTORY_PRESSURE})");
    if let (Some(before), Some(after)) = (rss_before, rss_after) {
        println!(
            "  rss_before_kib={} rss_after_kib={} delta_kib={}",
            before / 1024,
            after / 1024,
            after.saturating_sub(before) / 1024
        );
    }
    let ok = undo_before <= HISTORY_HARD_MAX && undo_after <= HISTORY_PRESSURE;
    println!("  memory → {}", if ok { "PASS" } else { "FAIL" });
    ok
}

fn bench_edit() -> bool {
    println!("[edit]");
    // Coalesced typing: one undo for a word — measures insert path cost.
    let typing = time_iters("type_1k_chars_coalesced", DEFAULT_WARMUP, DEFAULT_SAMPLES, || {
        let mut d = Document::new("");
        for i in 0..1_000 {
            let rev = d.revision();
            let _ = d.apply(
                rev,
                &[Edit {
                    start: i,
                    end: i,
                    replacement: "a".into(),
                }],
            );
        }
        assert_eq!(d.undo_len(), 1);
    });
    // Scatter inserts (no coalesce) — history-bound apply cost.
    let scatter = time_iters("scatter_512_inserts", DEFAULT_WARMUP, DEFAULT_SAMPLES, || {
        let mut d = Document::new(&"x".repeat(4_096));
        for _ in 0..512 {
            let rev = d.revision();
            let _ = d.apply(
                rev,
                &[Edit {
                    start: 0,
                    end: 0,
                    replacement: ".".into(),
                }],
            );
        }
    });
    let mut ok = print_stats(&typing, Some(50.0));
    ok &= print_stats(&scatter, Some(80.0));
    ok
}

fn bench_render() -> bool {
    println!("[render]");
    let body = {
        let mut lines = String::new();
        for i in 0..400 {
            lines.push_str(&format!(
                "fn example_{i}() {{ let value = {i} * 2; println!(\"{{value}}\"); }}\n"
            ));
        }
        lines
    };
    let mut app = EditorApp::new(&body);
    // Force a rust-ish language so highlight does real work when profiles exist.
    app.focused_mut().language = "rust".into();
    app.note_frame(120, 40);
    let stats = time_iters("draw_120x40", DEFAULT_WARMUP, DEFAULT_SAMPLES, || {
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| draw(frame, &app))
            .expect("draw");
    });
    // BUDGETS: p95 input-to-frame ≤16ms aspirational on modest sizes; 120x40 highlight
    // may exceed that — gate at 40ms for CI hosts under memory pressure.
    let mut ok = print_stats(&stats, Some(40.0));

    // ~1 MiB buffer: scroll+paint must stay cheap even under Urgent pressure.
    let big = "abcdefghijklmnopqrstuvwxyz 0123456789\n".repeat(28_000);
    let mut big_app = EditorApp::new(&big);
    big_app.note_frame(120, 40);
    big_app.force_memory_pressure(PressureLevel::Urgent);
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).expect("test backend");
    let _ = terminal.draw(|frame| draw(frame, &mut big_app));
    let scroll = time_iters("scroll_1mib_urgent", DEFAULT_WARMUP, DEFAULT_SAMPLES, || {
        big_app.focused_mut().scroll = big_app.focused().scroll.saturating_add(3) % 20_000;
        terminal
            .draw(|frame| draw(frame, &mut big_app))
            .expect("draw");
    });
    // Aspirational ≤16ms; gate 25ms under Urgent (must feel snappy, not "soft locked").
    ok &= print_stats(&scroll, Some(25.0));

    // Focused minimap RGB strip (always-on chrome): keep under a tight budget so
    // short-file always-visible does not tax every frame.
    let layout = crate::minimap_gfx::layout_proportional(400, 0, 40, 400, 1);
    let theme = crate::minimap_gfx::MinimapTheme::dark();
    let minimap = time_iters(
        "minimap_rgb_96x400",
        DEFAULT_WARMUP,
        DEFAULT_SAMPLES,
        || {
            let rgb = crate::minimap_gfx::render_base_rgb(layout, 96, 400, theme, |line, cols| {
                crate::minimap_gfx::plain_line_cells(
                    &format!("fn sample_{line}() {{ let x = {line}; }}"),
                    theme,
                )
                .into_iter()
                .take(cols)
                .collect()
            });
            assert_eq!(rgb.len(), 96 * 400 * 3);
        },
    );
    ok &= print_stats(&minimap, Some(25.0));
    ok
}

fn bench_open() -> bool {
    println!("[open]");
    let path = std::env::temp_dir().join(format!(
        "posit-bench-open-{}.txt",
        std::process::id()
    ));
    // ~1 MiB plain text — measures load + first paint without syntax overhead.
    let payload = "abcdefghijklmnopqrstuvwxyz 0123456789\n".repeat(28_000);
    std::fs::write(&path, &payload).expect("write fixture");
    let stats = time_iters("open_1mib_plain", DEFAULT_WARMUP, DEFAULT_SAMPLES, || {
        let text = std::fs::read_to_string(&path).expect("read");
        let mut app = EditorApp::new(&text);
        app.note_frame(80, 24);
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("backend");
        let _ = terminal.draw(|frame| draw(frame, &app));
    });
    let _ = std::fs::remove_file(&path);
    // BUDGETS: opening 1 MiB ≤100ms typical (release).
    print_stats(&stats, Some(100.0))
}

