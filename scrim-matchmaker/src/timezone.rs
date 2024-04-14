use std::str::FromStr;

use chrono::Utc;
use fuzzy_matcher::FuzzyMatcher;

use crate::{db, Context, Error};

async fn complete_zone(_ctx: Context<'_>, partial: &str) -> Vec<String> {
    let partial = partial.trim();
    let matcher = fuzzy_matcher::skim::SkimMatcherV2::default().element_limit(10);
    let mut zones: Vec<_> = chrono_tz::TZ_VARIANTS
        .iter()
        .map(|s| s.to_string())
        .filter(|s| matcher.fuzzy_match(s, partial).is_some())
        .collect();
    zones.sort_unstable_by_key(|tz| matcher.fuzzy_match(tz, partial));
    zones.reverse();
    zones
}

#[poise::command(
    slash_command,
    description_localized("en-US", "Set your timezone"),
    ephemeral
)]
#[tracing::instrument(err, skip(ctx), fields(user = ctx.author().id.get()))]
pub async fn timezone(
    ctx: Context<'_>,
    #[description = "Timezone to set. Use autocomplete to see available timezones"]
    #[autocomplete = "complete_zone"]
    zone: Option<String>,
) -> Result<(), Error> {
    if let Some(zone) = zone {
        let tz = chrono_tz::Tz::from_str(&zone).map_err(|_| "Invalid timezone")?;
        db::set_timezone(ctx, tz).await?;

        ctx.reply(format!(
            "Timezone set to `{}`. Current time: `{}`",
            tz,
            Utc::now().with_timezone(&tz).format("%d/%m/%Y %H:%M")
        ))
        .await?;
    } else {
        let tz = db::get_timezone(ctx).await?;
        ctx.reply(format!(
            "Your Timezone is `{}`. Current time: `{}`",
            tz,
            Utc::now().with_timezone(&tz).format("%d/%m/%Y %H:%M")
        ))
        .await?;
    }

    Ok(())
}
