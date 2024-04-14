use chrono::Utc;
use poise::CreateReply;

use crate::{db, lfs::LookingForScrim, Context, Error};

#[poise::command(
    slash_command,
    description_localized("en-US", "List your upcoming scrims"),
    ephemeral
)]
#[tracing::instrument(err, skip(ctx), fields(user = ctx.author().id.get()))]
pub async fn scrims(ctx: Context<'_>) -> Result<(), Error> {
    use std::fmt::Write;

    let mut scrims = db::get_future_scrims(ctx).await?;

    if scrims.is_empty() {
        ctx.reply("You have no upcoming scrims").await?;
        return Ok(());
    }

    scrims.sort_by_key(|scrim| scrim.time);

    let mut content = String::new();
    for scrim in scrims {
        content.push_str("## ");
        if let Some(team_name) = &scrim.team_name {
            write!(&mut content, "{team_name}: ")?;
        }
        write_scrim_meta(&scrim, None, &mut content)?;
        content.push('\n');

        if let Some(match_id) = scrim.match_id {
            let other = db::get_scrim(ctx, match_id).await?;
            content.push_str("Matched with ");
            write_scrim_with_name(&other, Some(&scrim), true, &mut content)?;
            content.push('\n');
        } else {
            let matches = db::find_matches(ctx, &scrim).await?;

            if matches.is_empty() {
                content.push_str("No matches found. Try again later\n");
            } else {
                content.push_str("### Potential matches:\n");
                for (_diff, lfs) in matches {
                    content.push_str("- ");
                    write_scrim_with_name(&lfs, Some(&scrim), true, &mut content)?;
                    content.push('\n');
                }
            }
        }
    }

    ctx.send(CreateReply::default().content(content)).await?;

    Ok(())
}

pub fn write_scrim_with_name(
    lfs: &LookingForScrim,
    other: Option<&LookingForScrim>,
    show_creator: bool,
    content: &mut String,
) -> Result<(), std::fmt::Error> {
    use std::fmt::Write;

    if show_creator {
        write!(content, "<@{}> ", lfs.creator_id)?;
        if let Some(team_name) = &lfs.team_name {
            write!(content, "(**{team_name}**) ")?;
        }
    } else if let Some(team_name) = &lfs.team_name {
        write!(content, "**{team_name}** ")?;
    }

    write_scrim_meta(lfs, other, content)?;

    Ok(())
}

pub fn write_scrim_meta(
    lfs: &LookingForScrim,
    other: Option<&LookingForScrim>,
    content: &mut String,
) -> Result<(), std::fmt::Error> {
    use std::fmt::Write;

    write!(content, "{:?}/{:?} {}", lfs.region, lfs.platform, lfs.range)?;

    let show_time = if let Some(other) = other {
        lfs.time != other.time
    } else {
        true
    };

    if show_time {
        write!(content, " on <t:{}:F>", lfs.time.timestamp(),)?;
        if lfs.time - Utc::now() < chrono::Duration::days(1) {
            write!(content, " (<t:{}:R>)", lfs.time.timestamp())?;
        }
    }

    Ok(())
}
