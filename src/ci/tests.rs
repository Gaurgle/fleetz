use super::*;

const RUNS: &str = include_str!("fixtures/runs.json");
const JOBS_FAILED: &str = include_str!("fixtures/jobs-failed.json");
const LOG_FAILED: &str = include_str!("fixtures/job-log-failed.txt");

fn utc(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .unwrap()
        .with_timezone(&Utc)
}

fn local(text: &str) -> DateTime<Local> {
    DateTime::parse_from_rfc3339(text)
        .unwrap()
        .with_timezone(&Local)
}

fn runs() -> Vec<WorkflowRun> {
    parse_runs(RUNS).unwrap()
}

fn finished_runs() -> Vec<WorkflowRun> {
    runs().into_iter().filter(|r| !r.is_live()).collect()
}

// --- Parsing, on payloads recorded against Gaurgle/fleetz ---

#[test]
fn the_runs_list_parses_into_the_spec_fields() {
    let runs = runs();
    assert_eq!(runs.len(), 3);
    let live = &runs[0];
    assert_eq!(live.id, 39590684061);
    assert_eq!(live.attempt, 1);
    assert_eq!(live.workflow.as_deref(), Some("ci"));
    assert_eq!(
        live.title,
        "feat(ui): date the failed GitHub attempt in the header"
    );
    assert_eq!(
        live.branch.as_deref(),
        Some("feat/79-date-the-failed-attempt")
    );
    assert_eq!(live.event, "push");
    assert_eq!(live.status.as_deref(), Some("in_progress"));
    assert_eq!(live.conclusion, None);
    assert_eq!(live.created, utc("2026-10-21T11:16:07Z"));
    assert_eq!(live.started, Some(utc("2026-10-21T11:16:07Z")));
    assert_eq!(live.updated, utc("2026-10-21T11:16:10Z"));
    assert_eq!(
        live.url,
        "https://github.com/Gaurgle/fleetz/actions/runs/39590684061"
    );
    assert!(live.is_live());
    assert_eq!(runs[1].conclusion.as_deref(), Some("success"));
    assert_eq!(runs[2].conclusion.as_deref(), Some("failure"));
    assert!(!runs[2].is_live());
}

#[test]
fn nullable_run_fields_parse_as_none() {
    let body = r#"{"workflow_runs":[{"id":1,"name":null,"display_title":"t",
            "head_branch":null,"event":"schedule","status":null,"conclusion":null,
            "created_at":"2026-10-21T11:16:07Z","updated_at":"2026-10-21T11:16:07Z",
            "html_url":"u"}]}"#;
    let run = &parse_runs(body).unwrap()[0];
    assert_eq!(run.workflow, None);
    assert_eq!(run.branch, None);
    assert_eq!(run.status, None);
    assert_eq!(run.started, None);
    assert_eq!(run.attempt, 1, "no run_attempt reads as the first");
    assert!(!run.is_live());
}

#[test]
fn the_jobs_list_parses_jobs_and_steps() {
    let jobs = parse_jobs(JOBS_FAILED).unwrap();
    assert_eq!(jobs.len(), 1);
    let job = &jobs[0];
    assert_eq!(job.id, 113914377501);
    assert_eq!(job.name, "check");
    assert_eq!(job.status, "completed");
    assert_eq!(job.conclusion.as_deref(), Some("failure"));
    assert_eq!(job.started, Some(utc("2026-10-09T16:19:46Z")));
    assert_eq!(job.completed, Some(utc("2026-10-09T16:20:40Z")));
    assert_eq!(job.steps.len(), 8);
    let step = failing_step(job).expect("one step failed");
    assert_eq!(step.number, 5);
    assert_eq!(step.name, "Run cargo clippy --all-targets -- -D warnings");
    assert_eq!(step.status, "completed");
    assert_eq!(step.started, Some(utc("2026-10-09T16:19:52Z")));
    assert_eq!(step.completed, Some(utc("2026-10-09T16:20:37Z")));
    assert_eq!(job.steps[5].conclusion.as_deref(), Some("skipped"));
}

#[test]
fn a_queued_job_without_steps_or_times_parses() {
    let body = r#"{"jobs":[{"id":7,"name":"check","status":"queued",
            "conclusion":null,"started_at":null,"completed_at":null}]}"#;
    let job = &parse_jobs(body).unwrap()[0];
    assert!(job.steps.is_empty());
    assert_eq!(job.started, None);
    assert_eq!(failing_step(job), None);
}

#[test]
fn a_body_that_does_not_parse_is_malformed() {
    let failure = parse_runs(r#"{"message":"Bad credentials"}"#).unwrap_err();
    assert_eq!(failure.kind, FailureKind::Malformed);
    assert_eq!(
        parse_jobs("<html>").unwrap_err().kind,
        FailureKind::Malformed
    );
}

// --- The log tail ---

#[test]
fn the_log_tail_is_the_failing_steps_last_twelve_lines() {
    let jobs = parse_jobs(JOBS_FAILED).unwrap();
    let tail = log_tail(LOG_FAILED, failing_step(&jobs[0]).unwrap());
    assert_eq!(
        tail,
        vec![
            "2697 | | > {",
            "     | |_^ the `Err`-variant is at least 128 bytes",
            "     |",
            "     = help: try reducing the size of `LoadError`, for example by boxing large elements or replacing it with `Box<LoadError>`",
            "     = help: for further information visit https://rust-lang.github.io/rust-clippy/rust-1.99.0/index.html#result_large_err",
            "     = note: `-D clippy::result-large-err` implied by `-D warnings`",
            "     = help: to override `-D warnings` add `#[allow(clippy::result_large_err)]`",
            "",
            "error: could not compile `fleetz` (bin \"fleetz\") due to 1 previous error",
            "warning: build failed, waiting for other jobs to finish...",
            "error: could not compile `fleetz` (bin \"fleetz\" test) due to 1 previous error",
            "##[error]Process completed with exit code 101.",
        ]
    );
    assert!(
        tail.iter().all(|l| !l.contains('\u{1b}')),
        "colours stripped"
    );
}

#[test]
fn the_log_tail_keeps_what_the_step_wrote_and_nothing_after_its_error() {
    // The cleanup steps start in the second the failing step ended; the
    // error line closes the step.
    let jobs = parse_jobs(JOBS_FAILED).unwrap();
    let tail = log_tail(LOG_FAILED, failing_step(&jobs[0]).unwrap());
    assert!(!tail.iter().any(|l| l.contains("Post job cleanup")));
    assert!(
        !tail.iter().any(|l| l.starts_with("2026-")),
        "stamps stripped"
    );
}

#[test]
fn the_log_tail_starts_at_the_steps_own_group_header() {
    let log = "\u{feff}2026-10-09T16:19:52.1Z tail of the step before\n\
               2026-10-09T16:19:52.2Z ##[group]Run make\n\
               2026-10-09T16:19:52.3Z make: *** [all] Error 1\n";
    let step = Step {
        number: 2,
        name: "Run make".to_string(),
        status: "completed".to_string(),
        conclusion: Some("failure".to_string()),
        started: Some(utc("2026-10-09T16:19:52Z")),
        completed: Some(utc("2026-10-09T16:19:52Z")),
    };
    assert_eq!(
        log_tail(log, &step),
        vec!["##[group]Run make", "make: *** [all] Error 1"]
    );
}

#[test]
fn a_step_that_never_ran_has_no_tail() {
    let step = Step {
        number: 1,
        name: "Run make".to_string(),
        status: "queued".to_string(),
        conclusion: None,
        started: None,
        completed: None,
    };
    assert!(log_tail(LOG_FAILED, &step).is_empty());
}

// --- Cadence ---

fn cadence(runs: &[WorkflowRun]) -> Cadence<'_> {
    Cadence {
        folded: false,
        has_identity: true,
        settled_for: DWELL,
        inflight: false,
        runs,
        last_attempt: None,
        pace: Pace::Unknown,
        not_before: None,
        now: local("2026-10-21T12:00:00+02:00"),
    }
}

#[test]
fn a_folded_pane_never_polls() {
    let c = Cadence {
        folded: true,
        ..cadence(&[])
    };
    assert!(!poll_due(&c));
}

#[test]
fn a_row_without_a_github_identity_never_polls() {
    let c = Cadence {
        has_identity: false,
        ..cadence(&[])
    };
    assert!(!poll_due(&c));
}

#[test]
fn the_first_poll_waits_for_the_cursor_to_dwell() {
    let moving = Cadence {
        settled_for: DWELL - std::time::Duration::from_millis(1),
        ..cadence(&[])
    };
    assert!(!poll_due(&moving), "a j run through the list sends nothing");
    assert!(poll_due(&cadence(&[])), "settled on a repo never polled");
    assert_eq!(DWELL, std::time::Duration::from_millis(300));
}

#[test]
fn a_poll_in_flight_is_not_stacked() {
    let c = Cadence {
        inflight: true,
        ..cadence(&[])
    };
    assert!(!poll_due(&c));
}

#[test]
fn a_live_run_polls_every_ten_seconds() {
    let runs = runs();
    let now = local("2026-10-21T12:00:00+02:00");
    let at = |secs| Cadence {
        last_attempt: Some(now - chrono::Duration::seconds(secs)),
        ..cadence(&runs)
    };
    assert_eq!(base_interval_secs(&runs), 10);
    assert!(!poll_due(&at(9)));
    assert!(poll_due(&at(10)));
}

#[test]
fn finished_runs_poll_every_minute() {
    let runs = finished_runs();
    let now = local("2026-10-21T12:00:00+02:00");
    let at = |secs| Cadence {
        last_attempt: Some(now - chrono::Duration::seconds(secs)),
        ..cadence(&runs)
    };
    assert_eq!(base_interval_secs(&runs), 60);
    assert_eq!(base_interval_secs(&[]), 60, "no runs at all");
    assert!(!poll_due(&at(59)));
    assert!(poll_due(&at(60)));
}

#[test]
fn a_queued_run_counts_as_live() {
    let mut runs = finished_runs();
    runs[0].status = Some("queued".to_string());
    assert_eq!(base_interval_secs(&runs), LIVE_SECS);
}

#[test]
fn a_low_budget_stretches_the_interval_and_a_pause_holds_it() {
    let runs = runs();
    let now = local("2026-10-21T12:00:00+02:00");
    assert_eq!(interval_secs(&runs, Pace::Every(25)), 25);
    assert_eq!(interval_secs(&runs, Pace::Every(3)), 10, "never faster");
    let stretched = Cadence {
        last_attempt: Some(now - chrono::Duration::seconds(20)),
        pace: Pace::Every(25),
        ..cadence(&runs)
    };
    assert!(!poll_due(&stretched));
    let paused = Cadence {
        pace: Pace::Paused {
            until: now + chrono::Duration::seconds(1),
        },
        ..cadence(&runs)
    };
    assert!(!poll_due(&paused), "even a repo never polled waits");
    let resumed = Cadence {
        pace: Pace::Paused {
            until: now - chrono::Duration::seconds(1),
        },
        ..cadence(&runs)
    };
    assert!(poll_due(&resumed));
}

#[test]
fn the_backoff_holds_the_poll() {
    let now = local("2026-10-21T12:00:00+02:00");
    let held = Cadence {
        not_before: Some(now + chrono::Duration::seconds(30)),
        ..cadence(&[])
    };
    assert!(!poll_due(&held));
    let over = Cadence {
        not_before: Some(now),
        ..cadence(&[])
    };
    assert!(poll_due(&over));
}

// --- The REST exchange, on the loopback ---

/// A loopback listener answering one connection per canned response, in
/// order, and handing back each request's head as it read it.
async fn serve(responses: Vec<String>) -> (String, tokio::sync::mpsc::UnboundedReceiver<String>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (sent, received) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        for response in responses {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut seen = Vec::new();
            let mut chunk = [0u8; 16 * 1024];
            while !String::from_utf8_lossy(&seen).contains("\r\n\r\n") {
                match socket.read(&mut chunk).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => seen.extend_from_slice(&chunk[..n]),
                }
            }
            let _ = sent.send(String::from_utf8_lossy(&seen).to_string());
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.shutdown().await;
        }
    });
    (format!("http://{addr}"), received)
}

fn response(status: &str, headers: &[&str], body: &str) -> String {
    let mut text = format!("HTTP/1.1 {status}\r\n");
    for header in headers {
        text.push_str(header);
        text.push_str("\r\n");
    }
    text.push_str(&format!(
        "content-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    ));
    text
}

const BUDGET: [&str; 5] = [
    "x-ratelimit-limit: 5000",
    "x-ratelimit-remaining: 4952",
    "x-ratelimit-used: 48",
    "x-ratelimit-reset: 1792583935",
    "x-ratelimit-resource: core",
];

fn test_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(2))
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap()
}

fn request(etag: Option<&str>, expanded: Option<ExpandedRequest>) -> CiRequest {
    CiRequest {
        key: (Some("gaurgle".to_string()), "fleetz".to_string()),
        owner: "gaurgle".to_string(),
        name: "fleetz".to_string(),
        attempt: 1,
        runs_etag: etag.map(str::to_string),
        expanded,
        tip: None,
    }
}

async fn run(request: CiRequest, root: &str) -> CiAnswer {
    poll_with(
        request,
        || async { Ok("tok".to_string()) },
        &test_client(),
        root,
    )
    .await
}

#[tokio::test]
async fn the_list_call_names_the_endpoint_and_returns_the_etag() {
    let mut headers = BUDGET.to_vec();
    headers.push("etag: W/\"6b13\"");
    let (root, mut seen) = serve(vec![response("200 OK", &headers, RUNS)]).await;
    let answer = run(request(None, None), &root).await;
    let head = seen.recv().await.unwrap();
    assert!(
        head.starts_with("GET /repos/gaurgle/fleetz/actions/runs?per_page=10 HTTP/1.1"),
        "{head}"
    );
    assert!(head.to_lowercase().contains("authorization: bearer tok"));
    assert!(!head.to_lowercase().contains("if-none-match"));
    assert_eq!(answer.failure, None);
    assert_eq!(
        answer.runs,
        Some(Fetched::Fresh {
            data: runs(),
            etag: Some("W/\"6b13\"".to_string())
        })
    );
    let budget = answer.budget.expect("the headers carried a reading");
    assert_eq!((budget.limit, budget.remaining), (5000, 4952));
    assert_eq!(answer.jobs, None, "no run expanded, no /jobs call");
}

#[tokio::test]
async fn the_etag_is_sent_back_and_a_304_is_not_modified() {
    let (root, mut seen) = serve(vec![response("304 Not Modified", &BUDGET, "")]).await;
    let answer = run(request(Some("W/\"6b13\""), None), &root).await;
    let head = seen.recv().await.unwrap().to_lowercase();
    assert!(head.contains("if-none-match: w/\"6b13\""), "{head}");
    assert_eq!(answer.failure, None);
    assert_eq!(answer.runs, Some(Fetched::NotModified));
    assert!(answer.budget.is_some(), "a 304 states the budget too");
}

fn expanded(run_id: u64) -> ExpandedRequest {
    ExpandedRequest {
        run_id,
        jobs_etag: None,
        held_jobs: Vec::new(),
        logs_held: Vec::new(),
    }
}

#[tokio::test]
async fn an_expanded_failed_run_reads_its_jobs_and_its_log_once() {
    let (root, mut seen) = serve(vec![
        response("304 Not Modified", &BUDGET, ""),
        response("200 OK", &["etag: W/\"jobs\""], JOBS_FAILED),
        response("200 OK", &[], LOG_FAILED),
    ])
    .await;
    let answer = run(
        request(Some("W/\"6b13\""), Some(expanded(37958308363))),
        &root,
    )
    .await;
    assert_eq!(answer.failure, None);
    let _list = seen.recv().await.unwrap();
    let jobs = seen.recv().await.unwrap();
    assert!(
        jobs.starts_with("GET /repos/gaurgle/fleetz/actions/runs/37958308363/jobs "),
        "{jobs}"
    );
    let log = seen.recv().await.unwrap();
    assert!(
        log.starts_with("GET /repos/gaurgle/fleetz/actions/jobs/113914377501/logs "),
        "{log}"
    );
    let (job_id, tail) = answer.log.expect("the failed job's log");
    assert_eq!(job_id, 113914377501);
    let LogTail::Lines(lines) = tail else {
        panic!("a log was read")
    };
    assert_eq!(lines.len(), LOG_TAIL_LINES);

    // Held, it is not asked for again.
    let (root, _seen) = serve(vec![
        response("304 Not Modified", &BUDGET, ""),
        response("304 Not Modified", &BUDGET, ""),
    ])
    .await;
    let again = ExpandedRequest {
        jobs_etag: Some("W/\"jobs\"".to_string()),
        held_jobs: parse_jobs(JOBS_FAILED).unwrap(),
        logs_held: vec![113914377501],
        ..expanded(37958308363)
    };
    let answer = run(request(Some("W/\"6b13\""), Some(again)), &root).await;
    assert_eq!(answer.failure, None);
    assert_eq!(answer.log, None);
    assert_eq!(
        answer.jobs,
        Some((37958308363, JobsAnswer::Fetched(Fetched::NotModified)))
    );
}

#[tokio::test]
async fn a_404_on_the_expanded_runs_jobs_says_the_run_is_gone() {
    let (root, _seen) = serve(vec![
        response("304 Not Modified", &BUDGET, ""),
        response("404 Not Found", &BUDGET, r#"{"message":"Not Found"}"#),
    ])
    .await;
    let answer = run(request(Some("W/\"6b13\""), Some(expanded(1))), &root).await;
    assert_eq!(answer.failure, None, "a gone run is not a failed poll");
    assert_eq!(answer.jobs, Some((1, JobsAnswer::Gone)));
}

#[tokio::test]
async fn a_primary_rate_limit_is_a_limited_failure_with_its_reading() {
    let headers = [
        "x-ratelimit-limit: 5000",
        "x-ratelimit-remaining: 0",
        "x-ratelimit-used: 5000",
        "x-ratelimit-reset: 1792583935",
    ];
    let body = r#"{"message":"API rate limit exceeded"}"#;
    let (root, _seen) = serve(vec![response("403 Forbidden", &headers, body)]).await;
    let answer = run(request(None, None), &root).await;
    let failure = answer.failure.expect("refused");
    assert!(failure.rate_limited);
    assert_eq!(failure.limit, Some(crate::github::RateLimit::Primary));
    assert_eq!(answer.runs, None, "an error body is never data");
    assert_eq!(answer.budget.unwrap().remaining, 0);
}

#[tokio::test]
async fn nothing_listening_is_offline() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let answer = run(request(None, None), &format!("http://{addr}")).await;
    assert_eq!(answer.failure.unwrap().kind, FailureKind::Offline);
    assert_eq!(answer.runs, None);
    assert_eq!(answer.budget, None);
}

// --- The session token (FZ-30c follow-up of #90) ---

#[tokio::test]
async fn the_token_is_looked_up_once_and_again_after_an_auth_failure() {
    let cache = TokenCache::new();
    let lookups = std::sync::atomic::AtomicUsize::new(0);
    let source = || async {
        let n = lookups.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok::<_, String>(format!("tok{n}"))
    };
    assert_eq!(cache.token(source).await.unwrap(), "tok0");
    assert_eq!(cache.token(source).await.unwrap(), "tok0", "held");
    assert_eq!(lookups.load(std::sync::atomic::Ordering::SeqCst), 1);

    let mut answer = poll_answer();
    answer.failure = Some(Failure::new(FailureKind::Offline, "no route"));
    cache.forget_after(&answer);
    assert_eq!(
        cache.token(source).await.unwrap(),
        "tok0",
        "offline keeps it"
    );

    answer.failure = Some(Failure::new(FailureKind::Refused, "Bad credentials"));
    cache.forget_after(&answer);
    assert_eq!(cache.token(source).await.unwrap(), "tok1", "asked again");
    answer.failure = Some(Failure::new(FailureKind::Auth, "not logged in"));
    cache.forget_after(&answer);
    assert_eq!(cache.token(source).await.unwrap(), "tok2");
}

#[tokio::test]
async fn a_failed_lookup_is_not_held() {
    let cache = TokenCache::new();
    let failed = cache
        .token(|| async { Err::<String, _>("not signed in".to_string()) })
        .await;
    assert!(failed.is_err());
    let token = cache.token(|| async { Ok::<_, String>("tok".to_string()) });
    assert_eq!(token.await.unwrap(), "tok");
}

fn poll_answer() -> CiAnswer {
    CiAnswer {
        key: (Some("gaurgle".to_string()), "fleetz".to_string()),
        attempt: 1,
        runs: None,
        jobs: None,
        log: None,
        full_log: None,
        failure: None,
        budget: None,
        tip: None,
    }
}

// --- The inspector's fetches (FZ-30f) ---

const CI_YML: &str = include_str!("fixtures/ci.yml");

#[test]
fn the_whole_log_is_every_line_without_stamps_or_colours() {
    let lines = log_lines(LOG_FAILED);
    assert_eq!(lines.len(), LOG_FAILED.lines().count());
    assert!(lines.iter().all(|l| !l.starts_with("2026-")), "stamps");
    assert!(lines.iter().all(|l| !l.contains('\u{1b}')), "colours");
    assert!(lines.iter().all(|l| !l.starts_with('\u{feff}')), "BOM");
    assert!(lines.contains(&"##[error]Process completed with exit code 101.".to_string()));
    assert!(
        lines.iter().any(|l| l.contains("Post job cleanup")),
        "the whole log, past the failing step"
    );
}

/// A runaway log is held as its last `LOG_LINE_CAP` lines under a line
/// that says so; one at the cap is held whole.
#[test]
fn a_whole_log_past_the_cap_keeps_its_last_lines() {
    let log = |n: usize| {
        (1..=n)
            .map(|i| format!("2026-10-09T16:20:37.5Z line {i}\n"))
            .collect::<String>()
    };
    let held = log_lines(&log(LOG_LINE_CAP + 3));
    assert_eq!(held.len(), LOG_LINE_CAP + 1);
    assert_eq!(held[0], LOG_TRUNCATED);
    assert_eq!(held[1], "line 4", "the first lines go");
    assert_eq!(held.last().unwrap(), &format!("line {}", LOG_LINE_CAP + 3));
    assert_eq!(
        LOG_TRUNCATED,
        format!(
            "log truncated to the last {LOG_LINE_CAP} lines; the whole log is on the run's page (O)"
        )
    );
    let whole = log_lines(&log(LOG_LINE_CAP));
    assert_eq!(whole.len(), LOG_LINE_CAP);
    assert_eq!(whole[0], "line 1", "no note at the cap");
}

#[tokio::test]
async fn the_poll_that_reads_a_tail_keeps_the_whole_log_too() {
    let (root, _seen) = serve(vec![
        response("304 Not Modified", &BUDGET, ""),
        response("200 OK", &[], JOBS_FAILED),
        response("200 OK", &[], LOG_FAILED),
    ])
    .await;
    let answer = run(
        request(Some("W/\"6b13\""), Some(expanded(37958308363))),
        &root,
    )
    .await;
    assert_eq!(
        answer.full_log,
        Some((113914377501, log_lines(LOG_FAILED))),
        "one download serves the tail and the log body"
    );
}

/// The failing step of the recorded failed run, matched in the
/// workflow file recorded at its commit (`93f4c8e`).
#[test]
fn the_failing_steps_block_is_found_by_its_run_text() {
    let lines: Vec<&str> = CI_YML.lines().collect();
    let jobs = parse_jobs(JOBS_FAILED).unwrap();
    let step = failing_step(&jobs[0]).unwrap();
    let block = step_block(CI_YML, &step.name).expect("matched");
    assert_eq!(block.len(), 1);
    assert_eq!(
        lines[block.start].trim(),
        "- run: cargo clippy --all-targets -- -D warnings"
    );
}

#[test]
fn a_uses_step_matches_past_its_comment_and_takes_its_with_block() {
    let lines: Vec<&str> = CI_YML.lines().collect();
    let name = "Run dtolnay/rust-toolchain@6bed0761d98439e5a578e2877258200ad565ba87";
    let block = step_block(CI_YML, name).expect("matched");
    assert!(lines[block.start].contains("- uses: dtolnay/rust-toolchain"));
    assert_eq!(block.len(), 4, "uses, with and its two keys");
    let checkout = step_block(CI_YML, "Run actions/checkout@v4").expect("matched");
    assert_eq!(
        checkout.len(),
        1,
        "the comment after it opens the next step"
    );
}

#[test]
fn a_named_step_matches_by_name_first_and_a_block_run_by_its_first_line() {
    let yaml = "name: Build\n\
                jobs:\n  \
                  build:\n    \
                    name: Build\n    \
                    steps:\n      \
                      - uses: actions/upload-artifact@v4\n        \
                        with:\n          \
                          name: Build\n      \
                      - name: \"Build\"\n        \
                        run: make\n\n      \
                      - run: |\n          \
                          cargo test\n          \
                          cargo doc\n";
    let lines: Vec<&str> = yaml.lines().collect();
    let named = step_block(yaml, "Build").expect("the step named Build");
    assert_eq!(lines[named.start].trim(), "- name: \"Build\"");
    assert_eq!(named.len(), 2, "trailing blank lines are not the block's");
    let block = step_block(yaml, "Run cargo test").expect("the block run");
    assert_eq!(lines[block.start].trim(), "- run: |");
    assert_eq!(block.end, lines.len());
    assert_eq!(
        step_block(yaml, "Run cargo bench"),
        None,
        "no match, no highlight"
    );
    assert_eq!(
        step_block("name: ci\n", "ci"),
        None,
        "the workflow's own name"
    );
}

#[test]
fn base64_is_decoded_by_hand_across_line_breaks() {
    assert_eq!(decode_base64("Zm9vYmFy").unwrap(), b"foobar");
    assert_eq!(decode_base64("Zm9v\nYg==\n").unwrap(), b"foob");
    assert_eq!(decode_base64("Zm8=").unwrap(), b"fo");
    assert_eq!(decode_base64("Zm9v!"), None);
    let body = r#"{"content":"bmFtZTog\nY2kK\n","encoding":"base64"}"#;
    assert_eq!(parse_contents(body).unwrap(), "name: ci\n");
    let large = parse_contents(r#"{"content":"","encoding":"none"}"#).unwrap_err();
    assert_eq!(
        Detail::<String>::from_result(Err(large), local("2026-10-21T12:00:00+02:00")),
        Detail::Unavailable("workflow file too large for the contents API".to_string())
    );
}

#[test]
fn a_detail_is_fetched_once_and_a_failed_one_again_after_a_minute() {
    let now = local("2026-10-21T12:00:00+02:00");
    let failed = Detail::<()>::Failed {
        message: "offline".to_string(),
        at: now,
    };
    assert!(Detail::<()>::wants_fetch(None, now));
    assert!(!Detail::wants_fetch(Some(&Detail::<()>::Loading), now));
    assert!(!Detail::wants_fetch(Some(&Detail::Ready(())), now));
    let gone = Detail::<()>::Unavailable("log not found".to_string());
    assert!(!Detail::wants_fetch(
        Some(&gone),
        now + chrono::Duration::days(1)
    ));
    assert!(!Detail::wants_fetch(
        Some(&failed),
        now + chrono::Duration::seconds(59)
    ));
    assert!(Detail::wants_fetch(
        Some(&failed),
        now + chrono::Duration::seconds(60)
    ));
    let offline = Failure::new(FailureKind::Offline, "no route");
    assert!(matches!(
        Detail::<()>::from_result(Err(offline), now),
        Detail::Failed { .. }
    ));
    let limited = Failure::rate_limit("API rate limit exceeded");
    assert!(matches!(
        Detail::<()>::from_result(Err(limited), now),
        Detail::Failed { .. }
    ));
}

#[tokio::test]
async fn the_script_comes_from_the_clone_and_github_only_without_it() {
    let asked = std::sync::atomic::AtomicBool::new(false);
    let remote = || async {
        asked.store(true, std::sync::atomic::Ordering::SeqCst);
        Ok::<_, Failure>("from GitHub".to_string())
    };
    let local = async { Some("from the clone".to_string()) };
    assert_eq!(local_first(local, remote).await.unwrap(), "from the clone");
    assert!(
        !asked.load(std::sync::atomic::Ordering::SeqCst),
        "not asked"
    );
    let lacking = async { None };
    assert_eq!(local_first(lacking, remote).await.unwrap(), "from GitHub");
    assert!(asked.load(std::sync::atomic::Ordering::SeqCst));
}

fn script_request(sha: &str, clone: Option<std::path::PathBuf>) -> DetailRequest {
    DetailRequest {
        key: (Some("gaurgle".to_string()), "fleetz".to_string()),
        owner: "gaurgle".to_string(),
        name: "fleetz".to_string(),
        want: DetailWant::Script {
            sha: sha.to_string(),
            path: ".github/workflows/ci.yml".to_string(),
            clone,
        },
    }
}

/// A clone with the commit answers from git and asks nothing of
/// GitHub, the token included; a sha the clone lacks goes to the
/// contents API at that ref.
#[tokio::test]
async fn a_script_falls_back_to_the_contents_api_on_a_sha_the_clone_lacks() {
    use std::process::Command as StdCommand;
    let dir = std::env::temp_dir().join(format!("fleetz_ci_script_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".github/workflows")).unwrap();
    let git = |args: &[&str]| {
        let out = StdCommand::new("git")
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "t@example.com"]);
    git(&["config", "user.name", "t"]);
    std::fs::write(dir.join(".github/workflows/ci.yml"), CI_YML).unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "ci"]);
    let sha = git(&["rev-parse", "HEAD"]);

    let answer = fetch_detail_with(
        script_request(&sha, Some(dir.clone())),
        || async { panic!("the clone had it: no token lookup") },
        &test_client(),
        "http://127.0.0.1:9",
    )
    .await;
    let DetailGot::Script { result, .. } = answer.got else {
        panic!("a script answer")
    };
    assert_eq!(result.unwrap(), CI_YML);
    assert_eq!(answer.budget, None, "no request went");

    let lacking = "93f4c8edaf3438b5413579aaa1b65f5c6c228b87";
    let body = r#"{"content":"bmFtZTog\nY2kK\n","encoding":"base64"}"#;
    let (root, mut seen) = serve(vec![response("200 OK", &BUDGET, body)]).await;
    let answer = fetch_detail_with(
        script_request(lacking, Some(dir.clone())),
        || async { Ok("tok".to_string()) },
        &test_client(),
        &root,
    )
    .await;
    let head = seen.recv().await.unwrap();
    assert!(
        head.starts_with(&format!(
            "GET /repos/gaurgle/fleetz/contents/.github/workflows/ci.yml?ref={lacking} "
        )),
        "{head}"
    );
    let DetailGot::Script { result, .. } = answer.got else {
        panic!("a script answer")
    };
    assert_eq!(result.unwrap(), "name: ci\n");
    assert!(answer.budget.is_some(), "the contents call bills core");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn the_log_body_reads_a_jobs_whole_log_and_its_tail_in_one_call() {
    let (root, mut seen) = serve(vec![response("200 OK", &BUDGET, LOG_FAILED)]).await;
    let jobs = parse_jobs(JOBS_FAILED).unwrap();
    let request = DetailRequest {
        want: DetailWant::Log {
            job_id: jobs[0].id,
            step: failing_step(&jobs[0]).cloned(),
        },
        ..script_request("", None)
    };
    let answer = fetch_detail_with(
        request,
        || async { Ok("tok".to_string()) },
        &test_client(),
        &root,
    )
    .await;
    let head = seen.recv().await.unwrap();
    assert!(
        head.starts_with("GET /repos/gaurgle/fleetz/actions/jobs/113914377501/logs "),
        "{head}"
    );
    let DetailGot::Log { result, tail, .. } = answer.got else {
        panic!("a log answer")
    };
    assert_eq!(result.unwrap(), log_lines(LOG_FAILED));
    let Some(LogTail::Lines(tail)) = tail else {
        panic!("the tail rides along")
    };
    assert_eq!(tail.len(), LOG_TAIL_LINES);
}

#[tokio::test]
async fn an_expired_log_is_unavailable_for_good() {
    let body = r#"{"message":"Gone"}"#;
    let (root, _seen) = serve(vec![response("410 Gone", &BUDGET, body)]).await;
    let request = DetailRequest {
        want: DetailWant::Log {
            job_id: 1,
            step: None,
        },
        ..script_request("", None)
    };
    let answer = fetch_detail_with(
        request,
        || async { Ok("tok".to_string()) },
        &test_client(),
        &root,
    )
    .await;
    let DetailGot::Log { result, .. } = answer.got else {
        panic!("a log answer")
    };
    let held = Detail::from_result(result, local("2026-10-21T12:00:00+02:00"));
    assert!(matches!(held, Detail::Unavailable(_)), "{held:?}");
}

#[tokio::test]
async fn no_token_is_an_auth_failure_and_sends_nothing() {
    let answer = poll_with(
        request(None, None),
        || async { Err("not authenticated with GitHub".to_string()) },
        &test_client(),
        "http://127.0.0.1:9",
    )
    .await;
    assert_eq!(answer.failure.unwrap().kind, FailureKind::Auth);
}

// --- The default branch's tip (FZ-83) ---

/// `commits/HEAD/status` for `Gaurgle/portfolio-site` on 2026-11-05,
/// trimmed: the rollup's red came from this status alone.
const TIP_STATUS: &str = r#"{
      "state": "failure",
      "sha": "cc524b4717d724cfe74fdde29c5f5ae3853e5cdf",
      "total_count": 1,
      "statuses": [{
        "context": "Vercel",
        "state": "failure",
        "description": "Aliasing has failed",
        "target_url": "https://vercel.com/gaurgles-projects/portfolio/DJX3"
      }]
    }"#;

/// The same commit's `check-runs`: one green Actions job.
const TIP_CHECKS: &str = r#"{
      "total_count": 1,
      "check_runs": [{
        "name": "build",
        "status": "completed",
        "conclusion": "success",
        "html_url": "https://github.com/Gaurgle/portfolio-site/actions/runs/1/job/2",
        "details_url": "https://github.com/Gaurgle/portfolio-site/actions/runs/1/job/2",
        "app": { "slug": "github-actions" },
        "output": { "title": null }
      }]
    }"#;

const TIP_SHA: &str = "cc524b4717d724cfe74fdde29c5f5ae3853e5cdf";

fn context(name: &str, state: CiStatus) -> Context {
    Context {
        name: name.to_string(),
        state,
        description: None,
        url: None,
        actions: false,
        source: source_of(name),
        started: None,
        completed: None,
    }
}

/// The Vercel case parses into a failing status context and a passing
/// Actions check run, each with what its row draws and opens.
#[test]
fn the_tips_status_and_check_runs_parse_into_contexts() {
    let status = parse_tip_status(TIP_STATUS).unwrap();
    assert_eq!(status.sha, TIP_SHA);
    assert!(status.complete);
    assert_eq!(
        status.statuses,
        vec![Context {
            name: "Vercel".to_string(),
            state: CiStatus::Failed,
            description: Some("Aliasing has failed".to_string()),
            url: Some("https://vercel.com/gaurgles-projects/portfolio/DJX3".to_string()),
            actions: false,
            source: "vercel".to_string(),
            started: None,
            completed: None,
        }]
    );
    let checks = parse_tip_checks(TIP_CHECKS).unwrap();
    assert!(checks.complete);
    assert_eq!(checks.runs.len(), 1);
    assert_eq!(checks.runs[0].name, "build");
    assert_eq!(checks.runs[0].state, CiStatus::Passed);
    assert!(checks.runs[0].actions, "an Actions check run");
    assert_eq!(checks.runs[0].source, "actions");
}

/// Where a context ran: `actions` for GitHub Actions, a known source
/// wherever its name says so, else the name up to its first `/` or `:`.
#[test]
fn a_contexts_source_is_actions_a_known_name_or_its_own() {
    assert_eq!(source_of("github-actions"), "actions");
    assert_eq!(source_of("Vercel"), "vercel");
    assert_eq!(source_of("ci/circleci: build"), "circleci");
    assert_eq!(source_of("netlify/site/deploy-preview"), "netlify");
    assert_eq!(source_of("codecov/patch"), "codecov");
    assert_eq!(
        source_of("continuous-integration/jenkins"),
        "continuous-integration"
    );
}

/// `commits/cc524b4/statuses` for `Gaurgle/portfolio-site` on
/// 2026-11-05, newest first as GitHub sends it, trimmed to the fields
/// read: three Vercel deployments of one commit.
const STATUS_HISTORY: &str = r#"[
      {"context": "Vercel", "state": "failure", "description": "Aliasing has failed",
       "target_url": "https://vercel.com/p/DJX3", "created_at": "2026-11-05T12:25:38Z"},
      {"context": "Vercel", "state": "success", "description": "Deployment has completed",
       "target_url": "https://vercel.com/p/FfkJ", "created_at": "2026-11-05T11:52:43Z"},
      {"context": "Vercel", "state": "pending", "description": "Vercel is deploying your app",
       "target_url": "https://vercel.com/p/FfkJ", "created_at": "2026-11-05T11:52:18Z"},
      {"context": "Vercel", "state": "pending", "description": "Vercel is deploying your app",
       "target_url": "https://vercel.com/p/DJX3", "created_at": "2026-11-05T11:24:27Z"},
      {"context": "Vercel", "state": "success", "description": "Deployment has completed",
       "target_url": "https://vercel.com/p/4U5y", "created_at": "2026-11-05T11:24:09Z"},
      {"context": "Vercel", "state": "pending", "description": "Vercel is deploying your app",
       "target_url": "https://vercel.com/p/4U5y", "created_at": "2026-11-05T11:23:52Z"}
    ]"#;

fn vercel_failed() -> Context {
    Context {
        description: Some("Aliasing has failed".to_string()),
        url: Some("https://vercel.com/p/DJX3".to_string()),
        ..context("Vercel", CiStatus::Failed)
    }
}

/// The rule: a status's duration is its latest job's own span, from the
/// latest `pending` to the first state after it that is not `pending`;
/// the age counts from the final state. On the recorded history the
/// latest `pending` is 11:52:18 and the deploy finished at 11:52:43
/// (25 s); the failure at 12:25:38, "Aliasing has failed", came half an
/// hour later and is the row's state and age, not part of its duration.
/// The issue's 3-state excerpt (pending 11:52:18, success 11:52:43,
/// failure 12:25:38) gives the same span. With no `pending` at all, no
/// start is known.
#[test]
fn a_status_row_spans_its_latest_attempt() {
    let history = parse_status_history(STATUS_HISTORY).unwrap();
    assert_eq!(history.len(), 6);
    let row = status_run(&vercel_failed(), &history);
    assert_eq!(row.name, "Vercel");
    assert_eq!(row.source, "vercel");
    assert_eq!(row.trigger, "status");
    assert_eq!(row.state, CiStatus::Failed);
    assert_eq!(row.title, "Aliasing has failed");
    assert_eq!(row.url.as_deref(), Some("https://vercel.com/p/DJX3"));
    assert_eq!(row.started, Some(utc("2026-11-05T11:52:18Z")));
    assert_eq!(row.finished, Some(utc("2026-11-05T11:52:43Z")));
    assert_eq!(row.at, Some(utc("2026-11-05T12:25:38Z")));

    let excerpt: Vec<StatusEvent> = history
        .iter()
        .filter(|e| e.at >= utc("2026-11-05T11:52:18Z"))
        .cloned()
        .collect();
    let row = status_run(&vercel_failed(), &excerpt);
    assert_eq!(row.started, Some(utc("2026-11-05T11:52:18Z")));
    assert_eq!(row.finished, Some(utc("2026-11-05T11:52:43Z")));

    let no_pending: Vec<StatusEvent> = history
        .iter()
        .filter(|e| e.state != CiStatus::Pending)
        .cloned()
        .collect();
    let row = status_run(&vercel_failed(), &no_pending);
    assert_eq!((row.started, row.finished), (None, None));
    assert_eq!(row.at, Some(utc("2026-11-05T12:25:38Z")));
}

/// While the final state is itself `pending`, the job is still running:
/// it started at the latest `pending` and has no finish yet. With no
/// history, the row has no times at all.
#[test]
fn a_pending_status_row_has_no_finish_and_no_history_no_times() {
    let history = parse_status_history(STATUS_HISTORY).unwrap();
    let running: Vec<StatusEvent> = history
        .iter()
        .filter(|e| e.at <= utc("2026-11-05T11:52:18Z"))
        .cloned()
        .collect();
    let pending = Context {
        state: CiStatus::Pending,
        ..vercel_failed()
    };
    let row = status_run(&pending, &running);
    assert_eq!(row.started, Some(utc("2026-11-05T11:52:18Z")));
    assert_eq!(row.finished, None);
    assert_eq!(row.at, Some(utc("2026-11-05T11:52:18Z")));
    let bare = status_run(&vercel_failed(), &[]);
    assert_eq!((bare.started, bare.finished, bare.at), (None, None, None));
}

/// A check run of another app keeps its own name, app, title and times.
#[test]
fn a_check_run_row_keeps_its_own_times() {
    let body = TIP_CHECKS.replace("github-actions", "netlify").replace(
        "\"output\": { \"title\": null }",
        "\"output\": { \"title\": \"Deploy preview ready\" }, \
             \"started_at\": \"2026-11-05T11:00:00Z\", \
             \"completed_at\": \"2026-11-05T11:01:30Z\"",
    );
    let check = parse_tip_checks(&body).unwrap().runs.remove(0);
    assert!(!check.actions);
    let row = check_run(&check);
    assert_eq!((row.source.as_str(), row.trigger), ("netlify", "check"));
    assert_eq!(row.title, "Deploy preview ready");
    assert_eq!(row.started, Some(utc("2026-11-05T11:00:00Z")));
    assert_eq!(row.finished, Some(utc("2026-11-05T11:01:30Z")));
    assert_eq!(row.at, Some(utc("2026-11-05T11:00:00Z")));
}

/// A page short of `total_count` is not every context, so the verdict
/// cannot be trusted over the fleet poll's.
#[test]
fn a_short_page_is_not_complete() {
    let body = TIP_CHECKS.replace("\"total_count\": 1", "\"total_count\": 101");
    assert!(!parse_tip_checks(&body).unwrap().complete);
    let body = TIP_STATUS.replace("\"total_count\": 1", "\"total_count\": 2");
    assert!(!parse_tip_status(&body).unwrap().complete);
}

/// A check run still going is pending; neutral and skipped pass as the
/// rollup counts them; anything else that concluded fails.
#[test]
fn a_check_runs_state_follows_the_rollup() {
    let with = |status: &str, conclusion: &str| {
        let body = TIP_CHECKS
            .replace(
                "\"status\": \"completed\"",
                &format!("\"status\": \"{status}\""),
            )
            .replace(
                "\"conclusion\": \"success\"",
                &format!("\"conclusion\": {conclusion}"),
            );
        parse_tip_checks(&body).unwrap().runs[0].state.clone()
    };
    assert_eq!(with("in_progress", "null"), CiStatus::Pending);
    assert_eq!(with("completed", "\"neutral\""), CiStatus::Passed);
    assert_eq!(with("completed", "\"skipped\""), CiStatus::Passed);
    assert_eq!(with("completed", "\"cancelled\""), CiStatus::Failed);
    assert_eq!(with("completed", "\"failure\""), CiStatus::Failed);
}

/// Any failure fails the tip, then any pending holds it; the culprit is
/// the first context that is not green, failures first, `+n` for the
/// rest; a green tip blames nothing and an empty one is unknown.
#[test]
fn the_tips_verdict_names_its_culprit() {
    let build = context("build", CiStatus::Passed);
    let vercel = context("Vercel", CiStatus::Failed);
    let deploy = context("deploy", CiStatus::Pending);
    let lint = context("lint", CiStatus::Failed);
    assert_eq!(
        tip_verdict([&build, &vercel].into_iter()),
        (CiStatus::Failed, Some("Vercel".to_string()))
    );
    assert_eq!(
        tip_verdict([&deploy, &build, &vercel, &lint].into_iter()),
        (CiStatus::Failed, Some("Vercel +2".to_string()))
    );
    assert_eq!(
        tip_verdict([&build, &deploy].into_iter()),
        (CiStatus::Pending, Some("deploy".to_string()))
    );
    assert_eq!(tip_verdict([&build].into_iter()), (CiStatus::Passed, None));
    assert_eq!(tip_verdict(std::iter::empty()), (CiStatus::Unknown, None));
}

fn tip_request(held: TipRequest) -> CiRequest {
    CiRequest {
        tip: Some(held),
        ..request(None, None)
    }
}

/// Asked for the tip, the poll reads the list, then `commits/HEAD/status`,
/// the check runs of the sha it named and, since the tip has a commit
/// status, that status's history, each answer with its `ETag`.
#[tokio::test]
async fn the_poll_reads_the_tips_status_and_its_check_runs() {
    let (root, mut seen) = serve(vec![
        response("200 OK", &BUDGET, RUNS),
        response("200 OK", &["etag: W/\"st\""], TIP_STATUS),
        response("200 OK", &["etag: W/\"cr\""], TIP_CHECKS),
        response("200 OK", &["etag: W/\"hi\""], STATUS_HISTORY),
    ])
    .await;
    let answer = run(tip_request(TipRequest::default()), &root).await;
    let _list = seen.recv().await.unwrap();
    let status = seen.recv().await.unwrap();
    let checks = seen.recv().await.unwrap();
    let history = seen.recv().await.unwrap();
    assert!(
        history.starts_with(&format!(
            "GET /repos/gaurgle/fleetz/commits/{TIP_SHA}/statuses?per_page=100 HTTP/1.1"
        )),
        "{history}"
    );
    assert!(
        status.starts_with("GET /repos/gaurgle/fleetz/commits/HEAD/status HTTP/1.1"),
        "{status}"
    );
    assert!(
        checks.starts_with(&format!(
            "GET /repos/gaurgle/fleetz/commits/{TIP_SHA}/check-runs?per_page=100 HTTP/1.1"
        )),
        "{checks}"
    );
    assert!(!status.to_lowercase().contains("if-none-match"));
    assert_eq!(answer.failure, None);
    assert_eq!(
        answer.tip,
        Some(TipAnswer {
            status: Fetched::Fresh {
                data: parse_tip_status(TIP_STATUS).unwrap(),
                etag: Some("W/\"st\"".to_string()),
            },
            history: Some((
                TIP_SHA.to_string(),
                Fetched::Fresh {
                    data: parse_status_history(STATUS_HISTORY).unwrap(),
                    etag: Some("W/\"hi\"".to_string()),
                }
            )),
            checks: Some((
                TIP_SHA.to_string(),
                Fetched::Fresh {
                    data: parse_tip_checks(TIP_CHECKS).unwrap(),
                    etag: Some("W/\"cr\"".to_string()),
                }
            )),
        })
    );
}

/// A tip with no commit statuses has no history to read: two calls.
#[tokio::test]
async fn a_tip_without_statuses_skips_the_history() {
    let bare =
        format!(r#"{{"state": "pending", "sha": "{TIP_SHA}", "total_count": 0, "statuses": []}}"#);
    let (root, _seen) = serve(vec![
        response("200 OK", &BUDGET, RUNS),
        response("200 OK", &[], &bare),
        response("200 OK", &[], TIP_CHECKS),
    ])
    .await;
    let answer = run(tip_request(TipRequest::default()), &root).await;
    assert_eq!(answer.failure, None, "no fourth call was made");
    let tip = answer.tip.expect("the tip");
    assert_eq!(tip.history, None);
    assert!(tip.checks.is_some());
}

/// Both reads are conditional: a 304 on the status keeps the held sha,
/// whose check runs are asked with their own `ETag`.
#[tokio::test]
async fn the_tip_reads_send_their_etags_and_a_304_keeps_the_held_sha() {
    let (root, mut seen) = serve(vec![
        response("304 Not Modified", &BUDGET, ""),
        response("304 Not Modified", &[], ""),
        response("304 Not Modified", &[], ""),
        response("304 Not Modified", &[], ""),
    ])
    .await;
    let held = TipRequest {
        sha: Some(TIP_SHA.to_string()),
        status_etag: Some("W/\"st\"".to_string()),
        checks_etag: Some("W/\"cr\"".to_string()),
        has_statuses: true,
        history_etag: Some("W/\"hi\"".to_string()),
    };
    let answer = run(
        CiRequest {
            runs_etag: Some("W/\"6b13\"".to_string()),
            ..tip_request(held)
        },
        &root,
    )
    .await;
    let _list = seen.recv().await.unwrap();
    let status = seen.recv().await.unwrap().to_lowercase();
    let checks = seen.recv().await.unwrap().to_lowercase();
    assert!(status.contains("if-none-match: w/\"st\""), "{status}");
    assert!(
        checks.contains(&format!("/commits/{TIP_SHA}/check-runs")),
        "{checks}"
    );
    assert!(checks.contains("if-none-match: w/\"cr\""), "{checks}");
    let history = seen.recv().await.unwrap().to_lowercase();
    assert!(
        history.contains(&format!("/commits/{TIP_SHA}/statuses")),
        "{history}"
    );
    assert!(history.contains("if-none-match: w/\"hi\""), "{history}");
    assert_eq!(answer.failure, None);
    assert_eq!(
        answer.tip,
        Some(TipAnswer {
            status: Fetched::NotModified,
            history: Some((TIP_SHA.to_string(), Fetched::NotModified)),
            checks: Some((TIP_SHA.to_string(), Fetched::NotModified)),
        })
    );
}

/// A new tip is another commit: its check runs are asked without the
/// old commit's `ETag`.
#[tokio::test]
async fn a_moved_tip_asks_its_check_runs_afresh() {
    let (root, mut seen) = serve(vec![
        response("200 OK", &BUDGET, RUNS),
        response("200 OK", &[], TIP_STATUS),
        response("200 OK", &[], TIP_CHECKS),
        response("200 OK", &[], STATUS_HISTORY),
    ])
    .await;
    let held = TipRequest {
        sha: Some("0ld".to_string()),
        status_etag: Some("W/\"st\"".to_string()),
        checks_etag: Some("W/\"cr\"".to_string()),
        has_statuses: true,
        history_etag: Some("W/\"hi\"".to_string()),
    };
    let answer = run(tip_request(held), &root).await;
    let _list = seen.recv().await.unwrap();
    let _status = seen.recv().await.unwrap();
    let checks = seen.recv().await.unwrap().to_lowercase();
    assert!(!checks.contains("if-none-match"), "{checks}");
    let history = seen.recv().await.unwrap().to_lowercase();
    assert!(!history.contains("if-none-match"), "{history}");
    assert_eq!(answer.failure, None);
}

/// A token that cannot read the tip's statuses still loads the expanded
/// run: the jobs and the failed job's log are read before the tip, whose
/// 403 is the attempt's outcome with all of them kept.
#[tokio::test]
async fn a_refused_tip_still_reads_the_expanded_runs_jobs_and_log() {
    let (root, mut seen) = serve(vec![
        response("200 OK", &BUDGET, RUNS),
        response("200 OK", &["etag: W/\"jobs\""], JOBS_FAILED),
        response("200 OK", &[], LOG_FAILED),
        response(
            "403 Forbidden",
            &[],
            r#"{"message":"Resource not accessible by integration"}"#,
        ),
    ])
    .await;
    let answer = run(
        CiRequest {
            expanded: Some(expanded(37958308363)),
            ..tip_request(TipRequest::default())
        },
        &root,
    )
    .await;
    let _list = seen.recv().await.unwrap();
    let jobs = seen.recv().await.unwrap();
    assert!(
        jobs.starts_with("GET /repos/gaurgle/fleetz/actions/runs/37958308363/jobs "),
        "{jobs}"
    );
    let _log = seen.recv().await.unwrap();
    let status = seen.recv().await.unwrap();
    assert!(
        status.starts_with("GET /repos/gaurgle/fleetz/commits/HEAD/status HTTP/1.1"),
        "{status}"
    );
    assert!(answer.runs.is_some());
    assert!(
        matches!(
            answer.jobs,
            Some((37958308363, JobsAnswer::Fetched(Fetched::Fresh { .. })))
        ),
        "{:?}",
        answer.jobs
    );
    assert_eq!(answer.log.map(|(id, _)| id), Some(113914377501));
    assert_eq!(answer.tip, None);
    assert!(answer.failure.is_some(), "the 403 is the outcome");
}

/// A repo with no commits has no tip: a 404 reads nothing more and
/// fails nothing.
#[tokio::test]
async fn a_repo_without_a_tip_is_not_a_failure() {
    let (root, _seen) = serve(vec![
        response("200 OK", &BUDGET, RUNS),
        response("404 Not Found", &[], "{}"),
    ])
    .await;
    let answer = run(tip_request(TipRequest::default()), &root).await;
    assert_eq!(answer.failure, None);
    assert_eq!(answer.tip, None);
    assert!(answer.runs.is_some());
}
