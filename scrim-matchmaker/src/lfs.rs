use std::{ops::Range, time::Duration};

use chrono::Utc;
use poise::{
    serenity_prelude::{
        self as serenity, ButtonStyle, CreateActionRow, CreateButton,
        CreateInteractionResponseMessage,
    },
    ChoiceParameter, CreateReply,
};

use crate::{db, Context, Error};

#[derive(ChoiceParameter, Debug, enum_utils::FromStr)]
pub enum Region {
    EU,
    NA,
}

#[derive(ChoiceParameter, Debug, enum_utils::FromStr)]
pub enum Platform {
    PC,
    Console,
}

fn parse_rank(s: &str) -> Result<u32, String> {
    let err = format!(
        "Invalid rank range. Must be formatted like `4.3k` or `4k-4.5k`. You entered: `{}`",
        s
    );
    let s = s.trim().trim_end_matches('k');
    let Ok(rank) = s.parse::<f64>() else {
        return Err(err);
    };
    Ok((rank * 1000.) as u32)
}

fn parse_rank_range(s: &str) -> Result<Range<u32>, String> {
    let s = s.trim();
    if let Some((from, to)) = s.split_once('-') {
        let from = parse_rank(from)?;
        let to = parse_rank(to)?;
        Ok(from..to)
    } else {
        let rank = parse_rank(s)?;
        Ok(rank..rank)
    }
}

#[derive(Debug)]
pub struct RankRange(pub Range<u32>);

#[derive(Debug)]
pub struct LookingForScrim {
    pub id: i32,
    pub creator_id: i64,
    pub team_name: Option<String>,
    pub region: Region,
    pub platform: Platform,
    pub range: RankRange,
    pub time: chrono::DateTime<Utc>,
    pub match_id: Option<i32>,
    pub cancelled: bool,
}

impl std::fmt::Display for RankRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0.start == self.0.end {
            write!(f, "{}k", self.0.start as f64 / 1000.)
        } else {
            write!(
                f,
                "{}k-{}k",
                self.0.start as f64 / 1000.,
                self.0.end as f64 / 1000.
            )
        }
    }
}

#[poise::command(
    slash_command,
    description_localized("en-US", "Look for a scrim"),
    ephemeral
)]
#[tracing::instrument(err, skip(ctx), fields(user = ctx.author().id.get()))]
pub async fn lfs(
    ctx: Context<'_>,
    #[description = "Region to look in"] region: Region,
    #[description = "Platform to look on"] platform: Platform,
    #[description = "Single rank or range of ranks to look for, e.g. `4.3k` or `4k-4.5k`"]
    range: String,
    #[description = "Start time, e.g. `20`, `8:30pm`, `tomorrow 8pm`, `20 monday` or `july 4th 20`"]
    time: String,
    #[description = "Optional team name to show in the confirmation message and to other users"]
    team_name: Option<String>,
) -> Result<(), Error> {
    let zone = db::get_timezone(ctx).await?;
    let now = Utc::now().with_timezone(&zone);

    let date = date_time_parser::DateParser::parse_relative(&time, now.date_naive())
        .unwrap_or(now.date_naive());
    let Some(time) = date_time_parser::TimeParser::parse_relative(&time, now.time()) else {
        return Err("No time specified. Please try again".into());
    };
    let time = chrono::NaiveDateTime::new(date, time);
    let Some(time) = time.and_local_timezone(zone).single() else {
        return Err("Invalid time".into());
    };
    let time = time.with_timezone(&Utc);

    let lfs = LookingForScrim {
        id: 0,
        creator_id: ctx.author().id.get() as i64,
        team_name,
        region,
        platform,
        range: RankRange(parse_rank_range(&range)?),
        time,
        match_id: None,
        cancelled: false,
    };

    let confirm_reply = CreateReply::default()
        .content(format!(
            "Looking for a scrim in {:?}/{:?} at {} on <t:{}:F>. Please confirm:",
            lfs.region,
            lfs.platform,
            lfs.range,
            lfs.time.timestamp()
        ))
        .components(vec![CreateActionRow::Buttons(vec![
            CreateButton::new("confirm")
                .style(ButtonStyle::Success)
                .label("Confirm"),
            CreateButton::new("cancel")
                .style(ButtonStyle::Danger)
                .label("Cancel"),
        ])]);
    let confirm_handle = ctx.send(confirm_reply).await?;
    let confirm_msg = confirm_handle.message().await?;

    if let Some(i) = confirm_msg
        .await_component_interaction(ctx)
        // One hour timeout before the message disappears
        .timeout(Duration::from_secs(60 * 60))
        .await
    {
        let confirmed = i.data.custom_id == "confirm";
        if confirmed {
            i.create_response(
                ctx,
                serenity::CreateInteractionResponse::UpdateMessage(
                    CreateInteractionResponseMessage::new()
                    .content(format!(
                        "Looking for a scrim in {:?}/{:?} at {} on <t:{}:F>\nUse `/scrims` to see potential matches.",
                        lfs.region,
                        lfs.platform,
                        lfs.range,
                        lfs.time.timestamp()
                    )).components(vec![])
                ),
            )
            .await?;
        } else {
            i.create_response(
                ctx,
                serenity::CreateInteractionResponse::UpdateMessage(
                    CreateInteractionResponseMessage::new()
                        .content("Cancelled")
                        .components(vec![]),
                ),
            )
            .await?;
            return Ok(()); // Cancelled
        }
    } else {
        confirm_handle.delete(ctx.into()).await?;
        return Ok(()); // Timeout
    }

    db::create_scrim(ctx, lfs).await?;

    Ok(())
}
