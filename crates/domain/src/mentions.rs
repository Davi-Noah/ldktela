//! Mention extraction.
//!
//! Mentions are parsed **on the server**, from the message body
//! (`docs/api/rest-api.md` §6.5). A client-supplied list would be trustworthy
//! only until someone opened DevTools, and the mention count drives
//! notifications.
//!
//! The wire syntax is not in the specification, so it is fixed here:
//!
//! * `<@01J8…>` — a user, by UUID
//! * `<@&01J8…>` — a role, by UUID
//! * `@everyone` — everyone, as a bare word
//!
//! The angle-bracket form exists so a mention cannot be produced by ordinary
//! prose: `escreva para @joao` mentions nobody.

use std::collections::BTreeSet;

use uuid::Uuid;

/// Everything a message body refers to. Sets, because the same mention repeated
/// in one message is one mention — `mentions` has no primary key in SRS §5.2, so
/// deduplication has to happen here or the table grows duplicates.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mentions {
    pub users: BTreeSet<Uuid>,
    pub roles: BTreeSet<Uuid>,
    pub everyone: bool,
}

impl Mentions {
    pub fn is_empty(&self) -> bool {
        self.users.is_empty() && self.roles.is_empty() && !self.everyone
    }
}

/// Extracts every mention from a message body.
///
/// Anything inside a fenced or inline code span is ignored: pasting a log line
/// that happens to contain a mention must not notify anyone.
pub fn extract(content: &str) -> Mentions {
    let mut out = Mentions::default();
    for segment in outside_code(content) {
        scan(segment, &mut out);
    }
    out
}

/// Splits the body into the parts that are **not** inside code, so a mention
/// pasted in a snippet stays inert.
fn outside_code(content: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut rest = content;
    let mut in_code = false;
    // Fences first: everything between ``` pairs is code, backticks included.
    while let Some(idx) = rest.find("```") {
        let (before, after) = rest.split_at(idx);
        if !in_code {
            segments.push(before);
        }
        rest = &after[3..];
        in_code = !in_code;
    }
    if !in_code {
        segments.push(rest);
    }

    // Then inline spans within each non-fenced segment.
    let mut out = Vec::new();
    for segment in segments {
        let mut inline = false;
        for piece in segment.split('`') {
            if !inline {
                out.push(piece);
            }
            inline = !inline;
        }
    }
    out
}

fn scan(segment: &str, out: &mut Mentions) {
    // Walking by char index, not by byte: message bodies are Portuguese, so a
    // byte cursor lands mid-codepoint on the first accented word and panics.
    let mut cursor = 0usize;
    while cursor < segment.len() {
        let rest = &segment[cursor..];
        let Some(current) = rest.chars().next() else {
            break;
        };

        if current == '<' {
            if let Some((mention, consumed)) = parse_bracketed(rest) {
                match mention {
                    Bracketed::User(id) => {
                        out.users.insert(id);
                    }
                    Bracketed::Role(id) => {
                        out.roles.insert(id);
                    }
                }
                cursor += consumed;
                continue;
            }
        }
        if rest.starts_with("@everyone") && is_boundary(segment, cursor + "@everyone".len()) {
            out.everyone = true;
            cursor += "@everyone".len();
            continue;
        }
        cursor += current.len_utf8();
    }
}

enum Bracketed {
    User(Uuid),
    Role(Uuid),
}

/// `<@uuid>` or `<@&uuid>`, returning the bytes consumed.
fn parse_bracketed(input: &str) -> Option<(Bracketed, usize)> {
    let rest = input.strip_prefix("<@")?;
    let (is_role, rest) = match rest.strip_prefix('&') {
        Some(rest) => (true, rest),
        None => (false, rest),
    };
    let end = rest.find('>')?;
    let id: Uuid = rest[..end].parse().ok()?;
    // `<@` + optional `&` + uuid + `>`
    let consumed = 2 + usize::from(is_role) + end + 1;
    Some((
        if is_role {
            Bracketed::Role(id)
        } else {
            Bracketed::User(id)
        },
        consumed,
    ))
}

/// `@everyone` has to end the token; `@everyonexyz` is not a mention.
fn is_boundary(text: &str, at: usize) -> bool {
    text[at..]
        .chars()
        .next()
        .is_none_or(|c| !c.is_alphanumeric() && c != '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u8) -> Uuid {
        Uuid::from_bytes([n; 16])
    }

    #[test]
    fn a_user_and_a_role_mention_are_distinguished_by_the_ampersand() {
        let content = format!("oi <@{}> e <@&{}>", id(1), id(2));
        let mentions = extract(&content);
        assert_eq!(mentions.users, BTreeSet::from([id(1)]));
        assert_eq!(mentions.roles, BTreeSet::from([id(2)]));
        assert!(!mentions.everyone);
    }

    #[test]
    fn the_same_mention_twice_is_one_mention() {
        // `mentions` não tem PK no SRS §5.2: sem deduplicar aqui, a tabela cria
        // linhas idênticas e o contador de menções conta duas vezes.
        let content = format!("<@{0}> <@{0}> <@{0}>", id(1));
        assert_eq!(extract(&content).users.len(), 1);
    }

    #[test]
    fn prose_never_produces_a_mention() {
        let mentions = extract("escreva para @joao sobre o @time");
        assert!(mentions.is_empty());
        assert!(extract("e-mail: alguem@exemplo.test").is_empty());
    }

    #[test]
    fn everyone_needs_a_token_boundary() {
        assert!(extract("aviso @everyone").everyone);
        assert!(extract("@everyone!").everyone);
        assert!(!extract("@everyonezinho").everyone);
        assert!(!extract("@everyone_teste").everyone);
    }

    #[test]
    fn a_mention_inside_code_is_inert() {
        let content = format!("veja `<@{}>` no log", id(1));
        assert!(
            extract(&content).is_empty(),
            "colar um log não pode notificar ninguém"
        );

        let fenced = format!("```\n<@{}>\n@everyone\n```", id(1));
        assert!(extract(&fenced).is_empty());

        // Fora do bloco continua valendo.
        let mixed = format!("```\n<@{}>\n```\ne também <@{}>", id(1), id(2));
        let mentions = extract(&mixed);
        assert_eq!(mentions.users, BTreeSet::from([id(2)]));
    }

    #[test]
    fn a_malformed_mention_is_plain_text() {
        assert!(extract("<@nao-e-uuid>").is_empty());
        assert!(extract("<@>").is_empty());
        assert!(extract(&format!("<@{}", id(1))).is_empty(), "sem fechar");
    }

    #[test]
    fn an_unclosed_code_fence_swallows_the_rest_of_the_message() {
        // Um bloco aberto e não fechado é código até o fim; é como o marked
        // renderiza, e a extração precisa concordar com o que o usuário vê.
        let content = format!("```\n<@{}>", id(1));
        assert!(extract(&content).is_empty());
    }
}
