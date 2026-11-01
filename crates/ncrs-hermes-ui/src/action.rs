use std::path::PathBuf;

use ncrs_filing::replan_work::{ReplanItem, ReplanScope};
use ncrs_filing::review_work::AcceptSummary;
use ncrs_index::RuleId;

use crate::message::HermesPage;
use crate::rule_form::RuleForm;

/// What the Hermes pages ask of the app, which they cannot do themselves. The
/// app runs the actions in order, right after `update`.
#[derive(Debug)]
pub enum Action {
    CloseContextMenu,
    /// Clears the error and the notice in the status bar.
    ClearStatus,
    ShowNotice(String),
    ShowError(String),
    RequestHermesCounts,
    /// Hermes reads these files again.
    ReadPathsAgain(Vec<PathBuf>),
    /// Opens the Rules page with `form` up.
    OpenRuleForm {
        form: Box<RuleForm>,
        return_to: Option<HermesPage>,
    },
    /// Leaves the plan for `page`, with the rule `id` selected there.
    OpenRuleOfPlan {
        id: RuleId,
        page: HermesPage,
    },
    ExcludeFromHermes(Vec<PathBuf>),
    /// The documents of the previewed replan were confirmed: one job files them.
    Refile {
        scope: ReplanScope,
        items: Vec<ReplanItem>,
    },
    /// The documents of a group were filed from the quick-fix menu.
    QuickFixFiled(AcceptSummary),
}
