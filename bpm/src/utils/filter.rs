use std::cmp::Ordering;
use std::collections::HashSet;

#[derive(PartialEq, Eq)]
pub enum Combination {
    All,
    Any,
}

pub fn select_list(
    select_list: Vec<String>,
    prompts: Vec<String>,
    combination: &Combination,
    case_sensitive: bool,
) -> Vec<String> {
    let prompts: HashSet<_> = if case_sensitive {
        prompts.into_iter().collect()
    } else {
        prompts.into_iter().map(|s| s.to_lowercase()).collect()
    };

    let is_valid = |s: &String| match combination {
        Combination::All => prompts.iter().all(|prompt| {
            let s = if case_sensitive {
                s.to_string()
            } else {
                s.to_lowercase()
            };
            s.contains(prompt)
        }),
        Combination::Any => prompts.iter().any(|prompt| {
            let s = if case_sensitive {
                s.to_string()
            } else {
                s.to_lowercase()
            };
            s.contains(prompt)
        }),
    };

    let result: Vec<_> = select_list.into_iter().filter(is_valid).collect();
    if result.is_empty() {
        return prompts.into_iter().collect();
    }
    result
}

pub fn sort_list(
    sort_list: Vec<String>,
    prompts: Vec<String>,
    combination: &Combination,
    case_sensitive: bool,
) -> Vec<String> {
    let prompts: HashSet<_> = if case_sensitive {
        prompts.into_iter().collect()
    } else {
        prompts.into_iter().map(|s| s.to_lowercase()).collect()
    };

    let is_valid = |s: &String| match combination {
        Combination::All => prompts.iter().all(|prompt| {
            let s = if case_sensitive {
                s.to_string()
            } else {
                s.to_lowercase()
            };
            s.contains(prompt)
        }),
        Combination::Any => prompts.iter().any(|prompt| {
            let s = if case_sensitive {
                s.to_string()
            } else {
                s.to_lowercase()
            };
            s.contains(prompt)
        }),
    };

    let mut result = sort_list;
    result.sort_by(|a, b| {
        let a_valid = is_valid(a);
        let b_valid = is_valid(b);
        match (a_valid, b_valid) {
            (true, true) | (false, false) => Ordering::Equal,
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
        }
    });
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_list() {
        let list: Vec<String> = ["12", "13", "23"]
            .map(std::string::ToString::to_string)
            .into();
        let prompts = vec!["1".to_string()];
        let result = select_list(list, prompts, &Combination::All, false);
        assert_eq!(result, vec!["12", "13"]);

        let list: Vec<String> = ["12", "13", "23", "34"]
            .map(std::string::ToString::to_string)
            .into();
        let prompts = ["1", "2"].map(std::string::ToString::to_string).into();
        let result = select_list(list, prompts, &Combination::Any, false);
        assert_eq!(result, vec!["12", "13", "23"]);

        let list = ["12", "13", "23", "34", "21"]
            .map(std::string::ToString::to_string)
            .into();
        let prompts = ["1", "2"].map(std::string::ToString::to_string).into();
        let result = select_list(list, prompts, &Combination::All, false);
        assert_eq!(result, vec!["12", "21"]);
    }

    #[test]
    fn test_sort_list() {
        let list = ["12", "13", "23"]
            .map(std::string::ToString::to_string)
            .into();
        let prompts = vec!["2".to_string()];
        let result = sort_list(list, prompts, &Combination::All, false);
        assert_eq!(result, vec!["12", "23", "13"]);
    }
}
