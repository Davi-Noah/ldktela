//! Proof that the permission bits written by hand in `domain::discord` are the
//! ones Discord actually uses.
//!
//! `domain` cannot depend on serenity: the workspace configures it with `client`
//! and `gateway`, which pull in tokio, and `CLAUDE.md` §3 keeps `domain` free of
//! that. So the constants are transcribed there and verified here, where
//! serenity is already present.
//!
//! If this file fails to compile because a constant was renamed, or fails at
//! runtime because a value moved, the fix belongs in `domain::discord` — not
//! here.

use domain::DiscordPermissions;
use serenity::model::permissions::Permissions;

#[test]
fn every_bit_matches_serenity() {
    assert_eq!(
        DiscordPermissions::ADMINISTRATOR,
        Permissions::ADMINISTRATOR.bits(),
        "ADMINISTRATOR divergiu"
    );
    assert_eq!(
        DiscordPermissions::STREAM,
        Permissions::STREAM.bits(),
        "STREAM divergiu"
    );
    assert_eq!(
        DiscordPermissions::VIEW_CHANNEL,
        Permissions::VIEW_CHANNEL.bits(),
        "VIEW_CHANNEL divergiu"
    );
    assert_eq!(
        DiscordPermissions::CONNECT,
        Permissions::CONNECT.bits(),
        "CONNECT divergiu"
    );
}

#[test]
fn the_four_bits_are_distinct() {
    let all = [
        DiscordPermissions::ADMINISTRATOR,
        DiscordPermissions::STREAM,
        DiscordPermissions::VIEW_CHANNEL,
        DiscordPermissions::CONNECT,
    ];
    for (i, a) in all.iter().enumerate() {
        for b in &all[i + 1..] {
            assert_ne!(a, b, "dois bits colidiram");
        }
    }
}
