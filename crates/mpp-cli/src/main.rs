use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use mpp_cli::{
    calc_line, check_report, proof_report, prove, run_file, run_lines, run_report_with,
    split_target, ProveArgs, ProveIo,
};
use mpp_export::{export_path, load, render_page, ExportOptions, Format, PageSize};

#[derive(Parser)]
#[command(
    name = "mpp",
    version,
    about = "Math++: write maths like a paper, and run it"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run a file and print the answer to each query
    Run {
        file: PathBuf,
        /// Show the working of `= ?!` queries under their answers
        #[arg(long)]
        steps: bool,
    },
    /// Check files for errors (exit code 1 when there are any)
    Check {
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Write a static HTML page with typeset maths, graphs and results
    Render {
        file: PathBuf,
        /// The page to write (default: the file's name with .html)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Export to PDF, LaTeX or HTML
    Export {
        file: PathBuf,
        /// pdf, tex or html
        #[arg(long, value_name = "FORMAT")]
        to: String,
        /// The file to write (default: the file's name with the new extension)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Leave the answers of queries out
        #[arg(long)]
        no_results: bool,
        /// Show the working of `= ?!` queries
        #[arg(long)]
        steps: bool,
        /// Show comments as notes
        #[arg(long)]
        comments: bool,
        /// A dark page (the default is light, for printing)
        #[arg(long)]
        dark: bool,
        /// PDF paper size
        #[arg(long, value_enum, default_value = "a4")]
        page: Paper,
        /// PDF margin in millimetres
        #[arg(long, default_value_t = 25.0)]
        margin: f32,
    },
    /// Ask the proof agent to prove a result: mpp prove file.mpp#Theorem-2
    Prove {
        /// The file, with the result after `#` (default: its first result)
        target: String,
        /// The most this run may cost, in dollars
        #[arg(long, default_value_t = 2.0)]
        cap: f64,
        /// Propose conjectures about this topic instead of proving
        #[arg(long, value_name = "TOPIC")]
        conjecture: Option<String>,
        /// Write the proposed change without asking
        #[arg(long)]
        yes: bool,
    },
    /// The proof agent's API key (kept in Windows Credential Manager)
    Key {
        #[command(subcommand)]
        action: KeyAction,
    },
    /// Export a file's theorems and proofs to a Lean 4 project
    Lean {
        file: PathBuf,
        /// The project folder (default: the file's name with -lean)
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Run `lake build` when Lean is installed
        #[arg(long)]
        build: bool,
    },
    /// Quick calculator: mpp calc "\int_0^1 x^2 dx"
    Calc {
        #[arg(required = true, num_args = 1..)]
        expression: Vec<String>,
    },
    /// Open the Math++ editor
    Ide { file: Option<PathBuf> },
}

#[derive(Subcommand)]
enum KeyAction {
    /// Store a key (read from standard input, never echoed or printed)
    Set,
    /// Remove the stored key
    Clear,
    /// Say whether a key is stored
    Status,
}

#[derive(Clone, Copy, ValueEnum)]
enum Paper {
    A4,
    Letter,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Run { file, steps } => run(&file, steps),
        Command::Check { files } => check(&files),
        Command::Render { file, output } => render(&file, output),
        Command::Export {
            file,
            to,
            output,
            no_results,
            steps,
            comments,
            dark,
            page,
            margin,
        } => {
            let opts = ExportOptions {
                include_results: !no_results,
                include_steps: steps,
                include_comments: comments,
                dark,
                page: match page {
                    Paper::A4 => PageSize::A4,
                    Paper::Letter => PageSize::Letter,
                },
                margin_mm: margin,
                slider_values: Vec::new(),
            };
            export(&file, &to, output, &opts)
        }
        Command::Prove {
            target,
            cap,
            conjecture,
            yes,
        } => prove_cmd(&target, cap, conjecture, yes),
        Command::Key { action } => key_cmd(&action),
        Command::Lean { file, out, build } => lean_cmd(&file, out, build),
        Command::Calc { expression } => calc(&expression.join(" ")),
        Command::Ide { file } => ide(file.as_deref()),
    }
}

fn fail(msg: impl std::fmt::Display) -> ExitCode {
    eprintln!("error: {msg}");
    ExitCode::FAILURE
}

fn run(file: &Path, steps: bool) -> ExitCode {
    match run_file(file) {
        Ok(ev) => {
            print!("{}", run_report_with(&ev, steps));
            if run_lines(&ev).iter().any(|l| l.error) {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(e) => fail(e),
    }
}

fn check(files: &[PathBuf]) -> ExitCode {
    let mut bad = false;
    for file in files {
        match run_file(file) {
            Ok(ev) => {
                let name = file.display().to_string();
                let report = check_report(&name, &ev);
                if report.is_empty() {
                    println!("{name}: ok");
                } else {
                    eprint!("{report}");
                }
                let src = std::fs::read_to_string(file).unwrap_or_default();
                let proofs = proof_report(&src, file.parent());
                if !proofs.is_empty() {
                    println!("{name}: proofs");
                    print!("{proofs}");
                }
                if !ev.errors().is_empty() {
                    bad = true;
                }
            }
            Err(e) => {
                eprintln!("error: {e}");
                bad = true;
            }
        }
    }
    if bad {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn render(file: &Path, output: Option<PathBuf>) -> ExitCode {
    let loaded = match load(file) {
        Ok(l) => l,
        Err(e) => return fail(e),
    };
    let out = output.unwrap_or_else(|| file.with_extension("html"));
    match std::fs::write(&out, render_page(&loaded)) {
        Ok(()) => {
            println!("wrote {}", out.display());
            ExitCode::SUCCESS
        }
        Err(e) => fail(format!("could not write {}: {e}", out.display())),
    }
}

fn export(file: &Path, to: &str, output: Option<PathBuf>, opts: &ExportOptions) -> ExitCode {
    let Some(format) = Format::parse(to) else {
        return fail(format!("unknown format `{to}`; use pdf, tex or html"));
    };
    let out = output.unwrap_or_else(|| file.with_extension(format.extension()));
    let exported = match export_path(file, format, opts) {
        Ok(e) => e,
        Err(e) => return fail(e),
    };
    if let Err(e) = std::fs::write(&out, &exported.main) {
        return fail(format!("could not write {}: {e}", out.display()));
    }
    let dir = out.parent().filter(|p| !p.as_os_str().is_empty());
    for (name, bytes) in &exported.files {
        let path = dir.map_or_else(|| PathBuf::from(name), |d| d.join(name));
        if let Err(e) = std::fs::write(&path, bytes) {
            return fail(format!("could not write {}: {e}", path.display()));
        }
    }
    println!("wrote {}", out.display());
    ExitCode::SUCCESS
}

/// The key store: Windows Credential Manager, or an empty one when
/// `MPP_NO_CREDENTIAL_STORE=1` (tests never touch the real entry).
fn key_store() -> Box<dyn mpp_proof::agent::KeyStore> {
    if std::env::var("MPP_NO_CREDENTIAL_STORE").is_ok_and(|v| v == "1") {
        Box::new(mpp_proof::agent::MemoryStore::default())
    } else {
        Box::new(mpp_proof::agent::CredentialStore::default())
    }
}

fn prove_cmd(target: &str, cap: f64, conjecture: Option<String>, yes: bool) -> ExitCode {
    let (path, name) = split_target(target);
    let args = ProveArgs {
        path: &path,
        target: name,
        conjecture,
        cap_usd: cap,
        yes,
    };
    let store = key_store();
    let mut out = std::io::stdout();
    let mut err = std::io::stderr();
    let mut answer = || {
        let mut s = String::new();
        let _ = std::io::stdin().read_line(&mut s);
        s
    };
    let mut io = ProveIo {
        out: &mut out,
        err: &mut err,
        answer: &mut answer,
    };
    let outcome = prove(
        &args,
        store.as_ref(),
        None,
        &mpp_proof::agent::AgentConfig::default(),
        &mut io,
    );
    ExitCode::from(outcome.code())
}

fn key_cmd(action: &KeyAction) -> ExitCode {
    let store = key_store();
    match action {
        KeyAction::Set => {
            eprint!("Paste the API key and press Enter (it is not shown): ");
            let mut s = String::new();
            if std::io::stdin().read_line(&mut s).is_err() || s.trim().is_empty() {
                return fail("no key was read");
            }
            match store.set(&mpp_proof::agent::ApiKey::new(s.trim())) {
                Ok(()) => {
                    eprintln!();
                    println!("key stored");
                    ExitCode::SUCCESS
                }
                Err(e) => fail(e),
            }
        }
        KeyAction::Clear => match store.delete() {
            Ok(()) => {
                println!("key removed");
                ExitCode::SUCCESS
            }
            Err(e) => fail(e),
        },
        KeyAction::Status => match store.get() {
            Ok(Some(_)) => {
                println!("set");
                ExitCode::SUCCESS
            }
            Ok(None) => {
                println!("not set");
                ExitCode::SUCCESS
            }
            Err(e) => fail(e),
        },
    }
}

fn lean_cmd(file: &Path, out: Option<PathBuf>, build: bool) -> ExitCode {
    let dir = out.unwrap_or_else(|| {
        let stem = file
            .file_stem()
            .map_or_else(|| "document".into(), |s| s.to_string_lossy().to_string());
        file.with_file_name(format!("{stem}-lean"))
    });
    match mpp_cli::prove::lean(file, &dir, build) {
        Ok(msg) => {
            print!("{msg}");
            ExitCode::SUCCESS
        }
        Err(e) => fail(e),
    }
}

fn calc(input: &str) -> ExitCode {
    match calc_line(input) {
        Ok(line) => {
            println!("{line}");
            ExitCode::SUCCESS
        }
        Err(e) => fail(e),
    }
}

/// Opens the Math++ editor (it is built into `mpp`) and waits until it is closed.
fn ide(file: Option<&Path>) -> ExitCode {
    let paths = file.map(Path::to_path_buf).into_iter().collect();
    match mpp_ide::run(paths) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(format!("could not start the editor: {e}")),
    }
}
