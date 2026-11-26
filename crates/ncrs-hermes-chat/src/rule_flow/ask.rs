//! What the model is asked about the levels, and what it answers.

use ncrs_ai::{
    file_names_share_structure, folder_depth, pattern_mismatches, AiError, ChatModel, Levels,
    ModelProblem, Revision,
};
use ncrs_index::{DocumentFields, Kinds};

/// What the model is asked.
#[derive(Debug, Clone)]
pub enum RuleAsk {
    Recognize {
        /// What the model is shown.
        files: Vec<(String, DocumentFields)>,
        /// All files of the folder: what the answer is checked against.
        all: Vec<(String, DocumentFields)>,
        kinds: Kinds,
    },
    /// The user told in words what to change about the levels.
    Revise {
        files: Vec<(String, DocumentFields)>,
        kinds: Kinds,
        previous: Levels,
        sentence: String,
    },
}

/// What the model made of it.
#[derive(Debug, Clone)]
pub enum RuleAnswer {
    Levels(Result<Levels, ModelProblem>),
    /// The levels after a sentence about them, and whether the user is done.
    Revised(Result<Revision, ModelProblem>),
    /// The thread that asked the model broke.
    Failed(ModelProblem),
}

impl RuleAsk {
    pub fn run(&self, model: &dyn ChatModel) -> RuleAnswer {
        match self {
            Self::Recognize { files, all, kinds } => RuleAnswer::Levels(
                recognize_checked(model, files, all, kinds)
                    .map_err(|error| ModelProblem::from(&error)),
            ),
            Self::Revise {
                files,
                kinds,
                previous,
                sentence,
            } => RuleAnswer::Revised(
                model
                    .revise_levels(files, kinds, previous, sentence)
                    .map_err(|error| ModelProblem::from(&error)),
            ),
        }
    }
}

/// The model's levels for the files. When they do not write the paths the
/// files have, or leave the file name automatic although the names follow a
/// pattern, the model is asked once more, and the answer with fewer
/// deviations is taken; a file name level wins over an automatic one.
fn recognize_checked(
    model: &dyn ChatModel,
    files: &[(String, DocumentFields)],
    all: &[(String, DocumentFields)],
    kinds: &Kinds,
) -> Result<Levels, AiError> {
    let depth = folder_depth(all.iter().map(|(path, _)| path.as_str()));
    let first = model.recognize_levels(files, kinds, depth)?;
    let misfits = pattern_mismatches(&first.pattern(), all, kinds);
    let name_missed = first.file.is_none() && file_names_share_structure(all);
    if misfits.is_empty() && !name_missed {
        return Ok(first);
    }
    let Ok(second) = model.recognize_levels_again(files, kinds, depth, &first, &misfits) else {
        return Ok(first);
    };
    let remaining = pattern_mismatches(&second.pattern(), all, kinds).len();
    let better = remaining < misfits.len()
        || (remaining == misfits.len() && name_missed && second.file.is_some());
    Ok(if better { second } else { first })
}
