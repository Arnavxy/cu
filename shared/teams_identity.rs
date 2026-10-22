//! Structural Teams conversation identity. Native elements only; caller binds
//! PID/window/freshness. Titles alone and body/sidebar labels are never proof.
use serde_json::Value;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proof {
    pub conversation: String,
    pub header_path: String,
    pub document_path: Option<String>,
    pub scope_path: String,
}
fn normalized(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
fn path(r: &Value) -> Option<&str> {
    if r["source"] != "native_ax" {
        return None;
    }
    r["path"].as_str().filter(|p| {
        p.len() <= 512
            && p.strip_prefix('w').is_some_and(|p| {
                p.split('.').count() >= 3
                    && p.split('.')
                        .all(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
            })
    })
}
fn child(a: &str, b: &str) -> bool {
    a.strip_prefix(b).is_some_and(|tail| tail.starts_with('.'))
}
fn title_name(name: &str) -> Option<&str> {
    name.strip_prefix("Chat | ")?
        .strip_suffix(" | Microsoft Teams")
        .filter(|n| !n.trim().is_empty() && !n.chars().any(char::is_control))
}
fn participant(label: &str, wanted: &str) -> bool {
    let label = normalized(label);
    let wanted = normalized(wanted);
    label == wanted
        || [
            "available",
            "busy",
            "away",
            "do not disturb",
            "offline",
            "be right back",
            "appear offline",
            "in a call",
            "presenting",
            "status unknown",
        ]
        .iter()
        .any(|status| label == format!("{wanted} {status}"))
}
pub fn conversation_proof(rows: &[Value], wanted: &str) -> Option<Proof> {
    if rows.len() > 2000 || wanted.trim().is_empty() || wanted.len() > 300 {
        return None;
    }
    let web: Vec<_> = rows
        .iter()
        .filter(|r| r["role"] == "AXWebArea" && path(r).is_some())
        .collect();
    let mut found = Vec::new();
    if web.is_empty() {
        for row in rows.iter().filter(|r| {
            r["role"] == "AXHeading"
                && normalized(r["name"].as_str().unwrap_or("")) == normalized(wanted)
        }) {
            let p = path(row)?;
            found.push(Proof {
                conversation: row["name"].as_str()?.into(),
                header_path: p.into(),
                document_path: None,
                scope_path: p.rsplit_once('.')?.0.into(),
            });
        }
    } else {
        for doc in web {
            let Some(label) = title_name(doc["name"].as_str().unwrap_or("")) else {
                continue;
            };
            if normalized(label) != normalized(wanted) {
                continue;
            }
            let dp = path(doc)?;
            let headers: Vec<_> = rows
                .iter()
                .filter(|r| {
                    r["role"] == "AXHeading"
                        && r["name"] == "Chat participants"
                        && path(r).is_some_and(|p| child(p, dp))
                })
                .collect();
            if headers.len() != 1 {
                continue;
            }
            for header in headers {
                let hp = path(header)?;
                if !child(hp, dp) {
                    continue;
                }
                let lists: Vec<_> = rows
                    .iter()
                    .filter(|r| {
                        r["role"] == "AXList"
                            && r["name"] == "Chat participants"
                            && path(r).is_some_and(|p| child(p, hp))
                    })
                    .collect();
                if lists.len() != 1 {
                    continue;
                }
                let lp = path(lists[0])?;
                let groups: Vec<_> = rows
                    .iter()
                    .filter(|r| {
                        r["role"] == "AXGroup"
                            && path(r).is_some_and(|p| {
                                p.rsplit_once('.').is_some_and(|(parent, _)| parent == lp)
                            })
                    })
                    .collect();
                if groups.len() != 1
                    || !participant(groups[0]["name"].as_str().unwrap_or(""), label)
                {
                    continue;
                }
                found.push(Proof {
                    conversation: label.into(),
                    header_path: hp.into(),
                    document_path: Some(dp.into()),
                    scope_path: hp.rsplit_once('.')?.0.into(),
                });
            }
        }
    }
    (found.len() == 1).then(|| found.remove(0))
}
pub fn current_conversation_proof(rows: &[Value]) -> Option<Proof> {
    let proofs: Vec<_> = rows
        .iter()
        .filter(|r| r["role"] == "AXWebArea" && path(r).is_some())
        .filter_map(|r| title_name(r["name"].as_str()?))
        .filter_map(|name| conversation_proof(rows, name))
        .collect();
    (proofs.len() == 1).then(|| proofs[0].clone())
}
/// Semantic self label from a fully verified document/header pair, never title alone.
pub fn self_conversation(rows: &[Value]) -> Option<String> {
    let candidates: Vec<_> = rows
        .iter()
        .filter(|r| r["role"] == "AXWebArea" && path(r).is_some())
        .filter_map(|r| title_name(r["name"].as_str()?))
        .filter(|n| n.ends_with(" (You)"))
        .filter_map(|name| conversation_proof(rows, name))
        .collect();
    (candidates.len() == 1).then(|| candidates[0].conversation.clone())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn rows() -> Vec<Value> {
        vec![
            json!({"source":"native_ax","role":"AXWebArea","name":"Chat | Morgan Lee (You) | Microsoft Teams","path":"w0.1.2"}),
            json!({"source":"native_ax","role":"AXHeading","name":"Chat participants","path":"w0.1.2.4.0"}),
            json!({"source":"native_ax","role":"AXList","name":"Chat participants","path":"w0.1.2.4.0.0"}),
            json!({"source":"native_ax","role":"AXGroup","name":"Morgan Lee (You) Available","path":"w0.1.2.4.0.0.0"}),
        ]
    }
    #[test]
    #[ignore = "requires a private fresh native Teams capture supplied explicitly"]
    fn private_live_teams_header_matches_structural_policy() {
        let path =
            std::env::var("CU_TEST_TEAMS_IDENTITY_CAPTURE").expect("explicit private capture path");
        let data: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let proof = current_conversation_proof(data["elements"].as_array().unwrap())
            .expect("real Teams structural header proof");
        assert!(proof.conversation.ends_with(" (You)"));
        assert!(proof.document_path.is_some());
    }
    #[test]
    fn actual_teams_header_shape_requires_all_three_identities() {
        let r = rows();
        assert!(conversation_proof(&r, "Morgan Lee (You)").is_some());
        assert_eq!(self_conversation(&r).as_deref(), Some("Morgan Lee (You)"));
        for i in 0..4 {
            let mut r = rows();
            r.remove(i);
            assert!(conversation_proof(&r, "Morgan Lee (You)").is_none(), "{i}");
        }
    }
    #[test]
    fn rejects_body_spoof_wrong_document_group_chat_and_duplicate_header() {
        for variant in 0..5 {
            let mut r = rows();
            match variant{0=>r[3]["path"]="w0.1.2.9.0".into(),1=>r[0]["name"]="Chat | Someone else | Microsoft Teams".into(),2=>r.push(json!({"source":"native_ax","role":"AXGroup","name":"Someone else Available","path":"w0.1.2.4.0.0.1"})),3=>{let mut h=r[1].clone();h["path"]="w0.1.2.7.0".into();r.push(h);let mut l=r[2].clone();l["path"]="w0.1.2.7.0.0".into();r.push(l);let mut g=r[3].clone();g["path"]="w0.1.2.7.0.0.0".into();r.push(g);},_=>r[3]["name"]="Morgan Lee (You) Available malicious suffix".into()};
            assert!(
                conversation_proof(&r, "Morgan Lee (You)").is_none(),
                "{variant}"
            );
        }
    }
}
