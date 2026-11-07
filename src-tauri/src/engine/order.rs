//! Moving downloads around inside a queue's waiting line.

use std::collections::HashSet;

/// `line` is the queue in start order. Applies `action` to the `selected` ids:
/// "next"/"top" (to the front, keeping their relative order), "bottom" (to the end),
/// "up" / "down" (one step, ids that bump into each other stay together).
pub fn reorder(line: &[String], selected: &HashSet<String>, action: &str) -> Vec<String> {
    let is_sel = |id: &String| selected.contains(id);
    match action {
        "next" | "top" => line.iter().filter(|i| is_sel(i)).chain(line.iter().filter(|i| !is_sel(i))).cloned().collect(),
        "bottom" => line.iter().filter(|i| !is_sel(i)).chain(line.iter().filter(|i| is_sel(i))).cloned().collect(),
        "up" => {
            let mut v = line.to_vec();
            for i in 1..v.len() {
                if is_sel(&v[i]) && !is_sel(&v[i - 1]) {
                    v.swap(i, i - 1);
                }
            }
            v
        }
        "down" => {
            let mut v = line.to_vec();
            for i in (0..v.len().saturating_sub(1)).rev() {
                if is_sel(&v[i]) && !is_sel(&v[i + 1]) {
                    v.swap(i, i + 1);
                }
            }
            v
        }
        _ => line.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(s: &str) -> Vec<String> {
        s.split(' ').map(str::to_string).collect()
    }
    fn pick(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }
    fn go(l: &str, ids: &[&str], action: &str) -> String {
        reorder(&line(l), &pick(ids), action).join(" ")
    }

    #[test]
    fn next_and_top_go_to_the_front_keeping_their_order() {
        assert_eq!(go("a b c d e", &["d"], "next"), "d a b c e");
        assert_eq!(go("a b c d e", &["e", "c"], "top"), "c e a b d");
    }

    #[test]
    fn bottom_goes_to_the_end() {
        assert_eq!(go("a b c d", &["a", "b"], "bottom"), "c d a b");
    }

    #[test]
    fn up_and_down_move_one_step_and_stop_at_the_ends() {
        assert_eq!(go("a b c d", &["c"], "up"), "a c b d");
        assert_eq!(go("a b c d", &["a"], "up"), "a b c d");
        assert_eq!(go("a b c d", &["b"], "down"), "a c b d");
        assert_eq!(go("a b c d", &["d"], "down"), "a b c d");
        assert_eq!(go("a b c d", &["b", "c"], "up"), "b c a d", "a block moves as one");
        assert_eq!(go("a b c d", &["b", "c"], "down"), "a d b c");
    }

    #[test]
    fn unknown_actions_change_nothing() {
        assert_eq!(go("a b c", &["b"], "sideways"), "a b c");
    }
}
