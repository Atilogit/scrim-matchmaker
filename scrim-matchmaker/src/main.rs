mod cancel;
mod db;
mod lfs;
mod scrims;
mod timezone;

use anyhow::Context as _;
use poise::serenity_prelude::{ClientBuilder, GatewayIntents};
use shuttle_runtime::SecretStore;
use shuttle_serenity::ShuttleSerenity;
use tracing::Level;
use tracing_subscriber::{
    filter::Targets, fmt::format::FmtSpan, layer::SubscriberExt, util::SubscriberInitExt,
};

struct Data {
    db: sqlx::PgPool,
}
type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::ApplicationContext<'a, Data, Error>;

#[shuttle_runtime::main]
async fn main(
    #[shuttle_runtime::Secrets] secret_store: SecretStore,
    #[shuttle_shared_db::Postgres] pool: sqlx::PgPool,
) -> ShuttleSerenity {
    let layer = tracing_subscriber::fmt::layer()
        .pretty()
        .with_file(false)
        .with_line_number(false)
        .without_time()
        .with_span_events(FmtSpan::NEW);
    let filter = Targets::new()
        .with_target("serenity", Level::WARN)
        .with_default(Level::INFO);

    tracing_subscriber::registry()
        .with(filter)
        .with(layer)
        .init();

    // Get the discord token set in `Secrets.toml`
    let discord_token = secret_store
        .get("DISCORD_TOKEN")
        .context("'DISCORD_TOKEN' was not found")?;

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![
                register(),
                lfs::lfs(),
                timezone::timezone(),
                scrims::scrims(),
                cancel::cancel(),
            ],
            on_error: |error| {
                Box::pin(async move {
                    match error {
                        poise::FrameworkError::Setup { error, .. } => {
                            tracing::error!("Error in user data setup: {error}");
                        }
                        poise::FrameworkError::Command { ctx, error, .. } => {
                            let error = error.to_string();
                            tracing::error!("An error occured in a command: {}", error);
                            if let Err(e) = ctx.say(error).await {
                                tracing::error!("Error while handling error: {e}");
                            }
                        }
                        _ => {
                            if let Err(e) = poise::builtins::on_error(error).await {
                                tracing::error!("Error while handling error: {e}");
                            }
                        }
                    }
                })
            },
            ..Default::default()
        })
        .setup(|_ctx, _ready, _framework| {
            Box::pin(async move {
                tracing::info!("Running migrations");
                sqlx::migrate!().run(&pool).await?;
                tracing::info!("Migrations done");
                Ok(Data { db: pool })
            })
        })
        .build();

    let client = ClientBuilder::new(discord_token, GatewayIntents::non_privileged())
        .framework(framework)
        .await
        .map_err(shuttle_runtime::CustomError::new)?;

    Ok(client.into())
}

#[poise::command(
    slash_command,
    ephemeral,
    hide_in_help,
    description_localized("en-US", "Register the application commands.")
)]
pub async fn register(ctx: Context<'_>) -> Result<(), Error> {
    poise::samples::register_application_commands_buttons(ctx.into()).await?;
    Ok(())
}
