//! Defines types used to interact between invoker and valuer
use crate::Status;
use bitflags::bitflags;
use pom::TestId;
use serde::{Deserialize, Serialize};

bitflags! {
    #[derive(Serialize, Deserialize)]
    pub struct TestVisibleComponents: u32 {
        /// Test input data
        const TEST_DATA = 1;
        /// Solution stdout & stderr
        const OUTPUT = 2;
        /// Test answer
        const ANSWER = 4;
        /// Test status
        const STATUS = 8;
    }
}

bitflags! {
    #[derive(Serialize, Deserialize)]
    pub struct SubtaskVisibleComponents: u32 {
        /// Score gained for this subtask
        const SCORE = 1;
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
pub struct JudgeLogTestRow {
    pub test_id: pom::TestId,
    pub status: Status,
    pub components: TestVisibleComponents,
}

#[derive(Debug, Serialize, Deserialize, Copy, Clone, Eq, PartialEq)]
pub struct SubtaskId(std::num::NonZeroU32);

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
pub struct JudgeLogSubtaskRow {
    pub subtask_id: SubtaskId,
    pub score: u32,
    pub components: SubtaskVisibleComponents,
}

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
pub enum JudgeLogKind {
    /// Contains all tests.
    /// Test can be omitted, if staring it was speculation.
    Full,
    /// Contains judge log for contestant
    /// Valuer should respect various restrictions specified in config.
    Contestant,
}

/// Judge log from valuer POV
#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
pub struct JudgeLog {
    pub kind: JudgeLogKind,
    pub tests: Vec<JudgeLogTestRow>,
    pub subtasks: Vec<JudgeLogSubtaskRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProblemInfo {
    pub test_count: u32,
}

#[derive(Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct TestDoneNotification {
    pub test_id: TestId,
    pub test_status: Status,
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug)]
pub enum ValuerResponse {
    Test {
        test_id: TestId,
        live: bool,
    },
    Finish {
        score: u32,
        treat_as_full: bool,
        judge_log: JudgeLog,
    },
    LiveScore {
        score: u32,
    },
}
