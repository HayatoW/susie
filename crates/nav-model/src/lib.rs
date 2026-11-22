//! ナビゲーションのモデル検査。
//!
//! 人が見る状態は `docs/evidence/S10/navigation.md` の図と同じです。
//! 検査は、その状態に、直前の操作と遅れの印を足して、操作の前後を比べます。

mod diagram;
mod properties;

use stateright::{Checker, Expectation, Model, Property};

use crate::diagram::{Action, DiagramState};
use crate::properties::{
    BACK_MOVES, BACK_NOOP, CAPABILITY_MATCHES_HISTORY, FAILURE_KEEPS_PLACE, FORWARD_MOVES,
    FORWARD_NOOP, INDEX_IN_RANGE, OPEN_CLEARS_FORWARD, REACH_BACK, REACH_BACK_BLOCKED,
    REACH_FAILURE, REACH_FORWARD, REACH_FORWARD_BLOCKED, REACH_MIDDLE_DONE, REACH_OPEN,
    REACH_RELOAD, REACH_RELOAD_EMPTY, REACH_STALE, REACH_STALE_AFTER_RESULT, REACH_SUCCESS,
    REACH_SUPERSEDE, RELOAD_KEEPS_HISTORY, RELOAD_NOOP, RESULT_KEEPS_STALE, SHOWN_ONLY_ON_RESULT,
    STALE_IGNORED, START_IS_EMPTY, SUCCESS_COMMITS_PAGE, SUPERSEDE_MARKS_STALE, back_moves,
    back_noop, capability_matches_history, failure_keeps_place, forward_moves, forward_noop,
    index_in_range, open_clears_forward, reach_back, reach_back_blocked, reach_failure,
    reach_forward, reach_forward_blocked, reach_middle_done, reach_open, reach_reload,
    reach_reload_empty, reach_stale, reach_stale_after_result, reach_success, reach_supersede,
    reload_keeps_history, reload_noop, result_keeps_stale, shown_only_on_result, stale_ignored,
    start_is_empty, success_commits_page, supersede_marks_stale,
};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct Step {
    action: Action,
    before: DiagramState,
    before_stale: bool,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct State {
    diagram: DiagramState,
    stale: bool,
    step: Option<Step>,
}

struct Navigation;

impl Model for Navigation {
    type State = State;
    type Action = Action;

    fn init_states(&self) -> Vec<Self::State> {
        vec![State {
            diagram: DiagramState::start(),
            stale: false,
            step: None,
        }]
    }

    fn actions(&self, state: &Self::State, actions: &mut Vec<Self::Action>) {
        diagram::push_actions(&state.diagram, state.stale, actions);
    }

    fn next_state(&self, last: &Self::State, action: Self::Action) -> Option<Self::State> {
        let (diagram, stale) = diagram::apply(&last.diagram, last.stale, action);
        Some(State {
            diagram,
            stale,
            step: Some(Step {
                action,
                before: last.diagram.clone(),
                before_stale: last.stale,
            }),
        })
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            Property::always(BACK_NOOP, |_, state| back_noop(state)),
            Property::always(FORWARD_NOOP, |_, state| forward_noop(state)),
            Property::always(RELOAD_NOOP, |_, state| reload_noop(state)),
            Property::always(OPEN_CLEARS_FORWARD, |_, state| open_clears_forward(state)),
            Property::always(BACK_MOVES, |_, state| back_moves(state)),
            Property::always(FORWARD_MOVES, |_, state| forward_moves(state)),
            Property::always(RELOAD_KEEPS_HISTORY, |_, state| reload_keeps_history(state)),
            Property::always(SUPERSEDE_MARKS_STALE, |_, state| {
                supersede_marks_stale(state)
            }),
            Property::always(STALE_IGNORED, |_, state| stale_ignored(state)),
            Property::always(SHOWN_ONLY_ON_RESULT, |_, state| shown_only_on_result(state)),
            Property::always(SUCCESS_COMMITS_PAGE, |_, state| success_commits_page(state)),
            Property::always(FAILURE_KEEPS_PLACE, |_, state| failure_keeps_place(state)),
            Property::always(RESULT_KEEPS_STALE, |_, state| result_keeps_stale(state)),
            Property::always(CAPABILITY_MATCHES_HISTORY, |_, state| {
                capability_matches_history(state)
            }),
            Property::always(START_IS_EMPTY, |_, state| start_is_empty(state)),
            Property::always(INDEX_IN_RANGE, |_, state| index_in_range(state)),
            Property::sometimes(REACH_OPEN, |_, state| reach_open(state)),
            Property::sometimes(REACH_BACK_BLOCKED, |_, state| reach_back_blocked(state)),
            Property::sometimes(REACH_BACK, |_, state| reach_back(state)),
            Property::sometimes(REACH_FORWARD_BLOCKED, |_, state| {
                reach_forward_blocked(state)
            }),
            Property::sometimes(REACH_FORWARD, |_, state| reach_forward(state)),
            Property::sometimes(REACH_RELOAD_EMPTY, |_, state| reach_reload_empty(state)),
            Property::sometimes(REACH_RELOAD, |_, state| reach_reload(state)),
            Property::sometimes(REACH_SUCCESS, |_, state| reach_success(state)),
            Property::sometimes(REACH_FAILURE, |_, state| reach_failure(state)),
            Property::sometimes(REACH_STALE, |_, state| reach_stale(state)),
            Property::sometimes(REACH_STALE_AFTER_RESULT, |_, state| {
                reach_stale_after_result(state)
            }),
            Property::sometimes(REACH_MIDDLE_DONE, |_, state| reach_middle_done(state)),
            Property::sometimes(REACH_SUPERSEDE, |_, state| reach_supersede(state)),
        ]
    }
}

pub struct Report {
    text: String,
    ok: bool,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.ok
    }

    pub fn render(&self) -> &str {
        &self.text
    }
}

pub fn check() -> Report {
    let checker = Navigation.checker().threads(1).spawn_bfs().join();
    let properties = Navigation.properties();
    let found = checker.discoveries();
    let mut ok = checker.is_done();
    let mut body = Vec::new();
    for property in &properties {
        let discovered = found.get(property.name);
        let holds = match property.expectation {
            Expectation::Always | Expectation::Eventually => discovered.is_none(),
            Expectation::Sometimes => discovered.is_some(),
        };
        if holds {
            body.push(format!("{}: 成り立ちます", property.name));
            continue;
        }
        ok = false;
        body.push(format!("{}: 成り立ちません", property.name));
        if let Some(path) = discovered {
            let actions = path
                .clone()
                .into_actions()
                .into_iter()
                .map(diagram::action_label)
                .collect::<Vec<_>>()
                .join(" → ");
            body.push(format!("反例: {actions}"));
        }
    }
    let mut lines = vec![
        "ナビゲーションのモデル検査".to_string(),
        format!("状態の数: {}", checker.unique_state_count()),
        format!("探索の深さ: {}", checker.max_depth()),
        format!("性質: {} 件", properties.len()),
        if ok {
            "結果: すべて成り立ちます".to_string()
        } else {
            "結果: 成り立たない性質があります".to_string()
        },
    ];
    if !checker.is_done() {
        lines.push("探索: 最後まで終わっていません".to_string());
    }
    lines.extend(body);
    lines.push(String::new());
    Report {
        text: lines.join("\n"),
        ok,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::properties::NAMES;

    #[test]
    fn properties_hold() {
        let report = check();
        assert!(report.ok(), "{}", report.render());
    }

    #[test]
    fn property_names_match_the_checker() {
        let names: Vec<_> = Navigation
            .properties()
            .into_iter()
            .map(|property| property.name)
            .collect();
        assert_eq!(names, NAMES);
    }

    #[test]
    fn diagram_lists_every_property_and_bound() {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/evidence/S10/navigation.md");
        let text = std::fs::read_to_string(&path).expect("read the navigation diagram");
        for name in NAMES {
            assert!(text.contains(name), "図に性質がありません: {name}");
        }
        for word in ["読み込み中", "完了", "失敗", "戻れる", "進める", "開始"] {
            assert!(text.contains(word), "図に状態がありません: {word}");
        }
        assert!(text.contains(&format!("{} 種類", diagram::URLS.len())));
        assert!(text.contains(&format!("{} 件", diagram::MAX_ENTRIES)));
    }
}
