//! Interprets given request graph
use std::{cmp::Ordering, fmt};

use invoker_api::invoke::{Action, InvokeRequest};

pub struct Interpreter<'a> {
    req: &'a InvokeRequest,
    completed_steps: Vec<usize>,
}

impl fmt::Debug for Interpreter<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Interpreter")
            .field("completed_steps", &self.completed_steps)
            .finish()
    }
}

impl<'a> Interpreter<'a> {
    pub fn new(req: &'a InvokeRequest) -> Self {
        Interpreter {
            req,
            completed_steps: Vec::new(),
        }
    }

    pub fn is_completed(&self) -> bool {
        self.completed_steps.len() == self.req.steps.len()
    }

    pub fn poll(&mut self, done: &[usize]) -> Vec<usize> {
        // recode these steps as completed, filtering out duplicates.
        self.completed_steps.extend(
            done.iter()
                .copied()
                .filter(|x| self.completed_steps.binary_search(x).is_err())
                .collect::<Vec<_>>(),
        );
        self.completed_steps.sort_unstable();

        let mut resp = Vec::new();
        // TODO optimize
        for step_id in 0..self.req.steps.len() {
            if self.can_run(step_id) {
                resp.push(step_id);
            }
        }
        resp
    }

    fn can_run(&self, step_id: usize) -> bool {
        if self.completed_steps.binary_search(&step_id).is_ok() {
            // already finished
            return false;
        }
        for other_step in 0..self.req.steps.len() {
            if self.completed_steps.binary_search(&other_step).is_ok() {
                continue;
            }
            match std::cmp::Ord::cmp(
                &self.req.steps[other_step].stage,
                &self.req.steps[step_id].stage,
            ) {
                Ordering::Less => {
                    // some earlier stage has not finished yet

                    return false;
                }
                Ordering::Equal => {
                    if order_in_phase(&self.req.steps[other_step].action)
                        < order_in_phase(&self.req.steps[step_id].action)
                    {
                        // other step can be required for our.
                        return false;
                    }
                }
                Ordering::Greater => (),
            }
        }
        true
    }
}

fn order_in_phase(s: &Action) -> u8 {
    match s {
        Action::CreateFile { .. }
        | Action::CreatePipe { .. }
        | Action::OpenFile { .. }
        | Action::OpenNullFile { .. } => 0,
        Action::CreateSandbox(..) => 1,
        Action::ExecuteCommand(..) => 2,
    }
}
