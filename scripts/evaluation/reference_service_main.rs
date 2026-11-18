fn main() -> std::process::ExitCode {
    let argv = std::env::args_os().skip(1).collect::<Vec<_>>();
    if argv.first().is_some_and(|s| s == "__native-runner") {
        return akasha_otsuge::adapter::run_guard(&argv[1..]).map_or(
            std::process::ExitCode::FAILURE,
            std::process::ExitCode::from,
        );
    }
    assert!(argv.is_empty());
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    match runtime.block_on(akasha_otsuge::daemon::reference_fixture::run()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("reference fixture {}", error.0);
            std::process::ExitCode::FAILURE
        }
    }
}
