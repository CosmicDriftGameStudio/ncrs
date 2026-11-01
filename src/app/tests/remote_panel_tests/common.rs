use super::*;

pub(in crate::app::tests::remote_panel_tests) const HOST: &str = "test.invalid";

pub(in crate::app::tests::remote_panel_tests) fn url(path: &str) -> PathBuf {
    Fixture::at(path).to_path_buf()
}

/// Delivers `message` and everything the tasks it starts report.
pub(in crate::app::tests::remote_panel_tests) async fn settle(app: &mut App, message: Message) {
    let mut queue = vec![message];
    while let Some(next) = queue.pop() {
        let Some(mut stream) = task::into_stream(app.update(next)) else {
            continue;
        };
        while let Some(action) = stream.next().await {
            // The timer of a notice is waited for, not delivered: the
            // notice is what a test looks at afterwards.
            if let Action::Output(reported) = action {
                if !matches!(reported, Message::NoticeExpired(_)) {
                    queue.push(reported);
                }
            }
        }
    }
}

/// Runs the tasks `task` starts to the end, and what they report.
pub(in crate::app::tests::remote_panel_tests) async fn run(app: &mut App, task: Task<Message>) {
    let Some(mut stream) = task::into_stream(task) else {
        return;
    };
    let mut reports = Vec::new();
    while let Some(action) = stream.next().await {
        if let Action::Output(reported) = action {
            reports.push(reported);
        }
    }
    for reported in reports {
        settle(app, reported).await;
    }
}

/// What the tasks `message` starts report, not delivered.
pub(in crate::app::tests::remote_panel_tests) async fn reports(
    app: &mut App,
    message: Message,
) -> Vec<Message> {
    let mut found = Vec::new();
    if let Some(mut stream) = task::into_stream(app.update(message)) {
        while let Some(action) = stream.next().await {
            if let Action::Output(reported) = action {
                if !matches!(reported, Message::NoticeExpired(_)) {
                    found.push(reported);
                }
            }
        }
    }
    found
}

/// Opens `path` in `side` and runs what that starts to the end.
pub(in crate::app::tests::remote_panel_tests) async fn open(
    app: &mut App,
    side: PanelSide,
    path: PathBuf,
    select: Option<String>,
) {
    let task = app.load(side, path, select);
    run(app, task).await;
}

pub(in crate::app::tests::remote_panel_tests) fn app_over(fixture: &Fixture) -> App {
    App::with_environment(Environment::isolated())
        .0
        .with_remote(Arc::clone(&fixture.pool))
}

pub(in crate::app::tests::remote_panel_tests) async fn type_into(
    app: &mut App,
    host: &str,
    user: &str,
    password: &str,
) {
    for message in [
        RemoteMessage::Field(ConnectField::Host, host.to_owned()),
        RemoteMessage::Field(ConnectField::User, user.to_owned()),
        RemoteMessage::Password(Secret::new(password.to_owned())),
    ] {
        settle(app, Message::Remote(message)).await;
    }
}

pub(in crate::app::tests::remote_panel_tests) fn names(app: &App, side: PanelSide) -> Vec<String> {
    app.panel(side)
        .entries
        .iter()
        .map(|entry| entry.name.to_string_lossy().into_owned())
        .collect()
}
