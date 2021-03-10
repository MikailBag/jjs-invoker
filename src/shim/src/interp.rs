//! Implements string interpolation

use std::collections::HashMap;

use invoker_api::{
    invoke::{Command, EnvVarValue, EnvironmentVariable},
    shim::RequestExtensions,
};

#[derive(Debug, Clone, thiserror::Error)]
pub(crate) enum InterpolateError {
    #[error("got '$(' while parsing var name or ')' outside of substitution")]
    BadSyntax,
    #[error("unknown key {key} in command template")]
    MissingKey { key: String },
}

/// Interpolates string by dictionary
///
/// Few examples of correct template strings:
/// - foo
/// - fo$(KeyName)
/// - fo$$$$(SomeKey)
///
/// Few examples of incorrect strings:
/// - $(
/// - $(SomeKey))
fn interpolate_string(
    string: &str,
    dict: &HashMap<String, String>,
) -> Result<String, InterpolateError> {
    let ak = aho_corasick::AhoCorasick::new_auto_configured(&["$(", ")"]);
    let matches = ak.find_iter(string);
    let mut out = String::new();
    let mut cur_pos = 0;
    let mut next_pat_id = 0;
    for m in matches {
        if m.pattern() != next_pat_id {
            return Err(InterpolateError::BadSyntax);
        }

        let chunk = &string[cur_pos..m.start()];
        cur_pos = m.end();
        if next_pat_id == 0 {
            out.push_str(chunk);
        } else {
            match dict.get(chunk) {
                Some(val) => {
                    out.push_str(val);
                }
                None => {
                    return Err(InterpolateError::MissingKey {
                        key: chunk.to_string(),
                    });
                }
            }
        }
        next_pat_id = 1 - next_pat_id;
    }
    let tail = &string[cur_pos..];
    out.push_str(tail);
    Ok(out)
}

/// Interpolates command fields (argv, env and cwd)
pub(crate) fn interpolate_command(
    command: &Command,
    dict: &HashMap<String, String>,
) -> Result<Command, InterpolateError> {
    let mut res = Command {
        argv: vec![],
        env: vec![],
        cwd: interpolate_string(&command.cwd, dict)?,
        stdio: command.stdio.clone(),
        sandbox_name: command.sandbox_name.clone(),
        ext: command.ext.clone(),
    };
    for arg in &command.argv {
        let interp = interpolate_string(arg, dict)?;
        res.argv.push(interp);
    }
    for env_var in command.env.iter() {
        let name = interpolate_string(&env_var.name, dict)?;
        let value = match &env_var.value {
            EnvVarValue::Plain(p) => EnvVarValue::Plain(interpolate_string(p, dict)?),
            _ => env_var.value.clone(),
        };
        res.env.push(EnvironmentVariable {
            name,
            value,
            ext: Default::default(),
        });
    }
    res.cwd = interpolate_string(&command.cwd, dict)?;

    Ok(res)
}

pub(crate) fn get_interpolation_dict(req_exts: &RequestExtensions) -> HashMap<String, String> {
    let mut dict = HashMap::new();
    dict.insert("Os.Name".to_string(), "Linux".to_string());
    for (k, v) in req_exts.substitutions.clone() {
        dict.insert(format!("Request.{}", k), v);
    }
    dict
}
