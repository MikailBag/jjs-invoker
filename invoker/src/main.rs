use cfg::Config;
use cfg_if::cfg_if;
use db::schema::{Submission, SubmissionState};
use diesel::{pg::PgConnection, prelude::*};
use invoker as lib;
use slog::*;
use std::sync;

cfg_if! {
if #[cfg(target_os="linux")] {
    fn check_system() -> bool {
        if let Some(err) = minion::linux_check_environment() {
            eprintln!("system configuration issue: {}", err);
            return false;
        }
        true
    }
} else {
    fn check_system() -> bool {
        true
    }
}
}

struct InvokeRequest {
    submission: Submission,
}

fn derive_standard_submission_info(
    cfg: &Config,
    submission: &Submission,
    invokation_id: &str,
) -> lib::SubmissionInfo {
    lib::SubmissionInfo::new(
        &cfg.sysroot,
        submission.id(),
        invokation_id,
        &submission.toolchain,
    )
}

fn handle_judge_task(
    task: InvokeRequest,
    cfg: &Config,
    conn: &PgConnection,
    logger: &slog::Logger,
) {
    use db::schema::submissions::dsl::*;

    let submission = task.submission.clone();

    let submission_info = derive_standard_submission_info(cfg, &submission, "TODO");

    let judging_status = lib::invoke(submission_info, logger, cfg);

    let target = submissions.filter(id.eq(task.submission.id() as i32));
    diesel::update(target)
        .set((
            state.eq(SubmissionState::Done),
            status.eq(&judging_status.code.to_string()),
            status_kind.eq(&judging_status.kind.to_string()),
        ))
        .execute(conn)
        .expect("Db query failed");
    debug!(logger, "judging finished"; "outcome" => ?judging_status);
}

fn main() {
    use db::schema::submissions::dsl::*;
    dotenv::dotenv().ok();

    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();

    let root = Logger::root(drain, o!("app"=>"jjs:invoker"));

    info!(root, "starting");

    let config = cfg::get_config();
    let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL - must contain postgres URL");
    let db_conn = diesel::pg::PgConnection::establish(db_url.as_str())
        .unwrap_or_else(|_e| panic!("Couldn't connect to {}", db_url));
    let should_run = sync::Arc::new(sync::atomic::AtomicBool::new(true));
    {
        let should_run = sync::Arc::clone(&should_run);
        ctrlc::set_handler(move || {
            should_run.store(false, sync::atomic::Ordering::SeqCst);
        })
        .unwrap();
    }

    if check_system() {
        debug!(root, "system check passed")
    } else {
        return;
    }

    loop {
        if !should_run.load(sync::atomic::Ordering::SeqCst) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
        let waiting_submission = submissions
            .filter(state.eq(SubmissionState::WaitInvoke))
            .limit(1)
            .load::<Submission>(&db_conn)
            .expect("db error");
        let waiting_submission = waiting_submission.get(0);
        let waiting_submission = match waiting_submission {
            Some(s) => s.clone(),
            None => continue,
        };
        let ivr = InvokeRequest {
            submission: waiting_submission,
        };
        handle_judge_task(ivr, &config, &db_conn, &root);
    }
}
