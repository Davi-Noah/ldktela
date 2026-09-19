//! The `/tela` slash command: how a Discord account becomes a session (RF-01).

use api::AppState;
use domain::pairing::PairingCode;
use serenity::builder::{
    CreateCommand, CreateInteractionResponse, CreateInteractionResponseMessage,
};
use serenity::model::application::CommandInteraction;
use serenity::prelude::*;
use time::OffsetDateTime;
use uuid::Uuid;

pub const COMMAND_NAME: &str = "tela";

/// Para onde mandar quem quer hospedar a própria instância.
const REPOSITORY: &str = "https://github.com/gbrlevi/ldktela";

pub fn command() -> CreateCommand {
    CreateCommand::new(COMMAND_NAME)
        .description("Gera um código para conectar o aplicativo de compartilhamento de tela")
}

/// Handles the command and answers ephemerally.
///
/// Every reply is ephemeral, without exception: a pairing code posted where the
/// channel can read it is a pairing code anyone in the channel can use.
pub async fn handle(state: &AppState, ctx: &Context, command: &CommandInteraction) {
    let text = match build_reply(state, command).await {
        Ok(text) => text,
        Err(error) => {
            tracing::error!(%error, "issuing pairing code");
            "Não consegui gerar o código agora. Tente de novo em instantes.".to_owned()
        }
    };

    let response = CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new()
            .content(text)
            .ephemeral(true),
    );
    if let Err(error) = command.create_response(&ctx.http, response).await {
        tracing::error!(%error, "answering the pairing command");
    }
}

async fn build_reply(state: &AppState, command: &CommandInteraction) -> anyhow::Result<String> {
    let Some(guild_id) = command.guild_id else {
        return Ok(
            "Rode este comando dentro do servidor cujo compartilhamento você quer usar.".to_owned(),
        );
    };

    // A instancia hospedada serve os guilds que ela nomeia (ADR-0035). Recusar
    // aqui, com o motivo e a saida, e o que separa "nao e para voce" de "esta
    // quebrado" — o guild nem chegou a ser espelhado, entao tudo depois disto
    // falharia fechado sem explicar nada.
    if !state.config.discord.serves(guild_id.get()) {
        return Ok(format!(
            "Este servidor não está autorizado a usar esta instância do ldktela.
             O ldktela é software livre: para usar no seu servidor, hospede a sua própria a              partir de {REPOSITORY}."
        ));
    }

    let discord_user_id = i64::try_from(command.user.id.get())?;
    let discord_guild_id = i64::try_from(guild_id.get())?;
    let now = OffsetDateTime::now_utc();

    // Limite de emissao, nao de tentativa. O codigo em si e forte; o que precisa
    // de freio e alguem pedindo codigos em massa para uma conta que controla.
    let window = now - time::Duration::hours(1);
    let issued =
        db::repo::pairing::recent_request_count(&state.pool, discord_user_id, window).await?;
    if issued >= state.config.discord.pairing_max_per_hour {
        return Ok("Você pediu códigos demais na última hora. Tente mais tarde.".to_owned());
    }

    // A conta local nasce aqui, nao no pareamento: assim `/auth/pair` so precisa
    // resolver o id, e o perfil ja esta atualizado quando a sessao abre.
    let display = command.user.global_name.clone();
    db::repo::users::upsert_from_discord(
        &state.pool,
        Uuid::now_v7(),
        discord_user_id,
        &command.user.name,
        display.as_deref(),
        command.user.avatar_url().as_deref(),
    )
    .await?;

    let code = PairingCode::generate();
    let ttl = state.config.discord.pairing_code_ttl;
    db::repo::pairing::insert(
        &state.pool,
        Uuid::now_v7(),
        &code.hash(),
        discord_user_id,
        discord_guild_id,
        now + time::Duration::seconds(ttl.as_secs() as i64),
    )
    .await?;

    let minutes = ttl.as_secs() / 60;
    Ok(format!(
        "Seu código é **{}**\n\nAbra o aplicativo e digite esse código. \
         Ele vale por {minutes} minutos e só pode ser usado uma vez.",
        code.expose()
    ))
}
