use crate::app::*;
use crate::context_menu::{ContextAction, HermesAction};
use crate::fs::remote::credentials::InMemoryPasswords;
use crate::fs::remote::session::ConnectFailure;
use crate::fs::remote::test_server::{Fixture, HOME, PASSWORD, USER};
use crate::fs::remote::Secret;
use crate::messages::{ConnectField, SettingsMessage};
use crate::remote_dialog;
use iced_test::futures::futures::StreamExt as _;
use iced_test::runtime::{task, Action};

mod common;
mod dialog;
mod saved_servers;
mod transfers;
