use chrono::Utc;
use poise::{
    serenity_prelude::{
        self as serenity,
        futures::{stream, StreamExt, TryStreamExt},
        ButtonStyle, ComponentInteraction, ComponentInteractionCollector, CreateActionRow,
        CreateButton, CreateInteractionResponse, CreateInteractionResponseMessage,
    },
    CreateReply,
};

use crate::{db, lfs::LookingForScrim, Context, Error};

struct ScrimMsg {
    index: usize,
    scrim: LookingForScrim,
    state: ScrimState,
}

enum ScrimState {
    Looking {
        previous_revoked: bool,
        matches: Vec<(f32, LookingForScrim)>,
    },
    Matched(LookingForScrim),
    Cancelled,
}

impl ScrimMsg {
    async fn new(ctx: Context<'_>, scrim: LookingForScrim, index: usize) -> Result<Self, Error> {
        if let Some(match_id) = scrim.match_id {
            let other = db::get_scrim(ctx, match_id).await?;
            if let Some(other_match_id) = other.match_id {
                if other_match_id != scrim.id {
                    db::revoke_scrim(ctx, scrim.id).await?;
                    return Ok(Self {
                        state: ScrimState::Looking {
                            previous_revoked: true,
                            matches: db::find_matches(ctx, &scrim).await?,
                        },
                        scrim,
                        index,
                    });
                }
            }
            Ok(Self {
                state: ScrimState::Matched(other),
                scrim,
                index,
            })
        } else {
            Ok(Self {
                state: ScrimState::Looking {
                    previous_revoked: false,
                    matches: db::find_matches(ctx, &scrim).await?,
                },
                scrim,
                index,
            })
        }
    }

    fn msg(&self, ctx: Context<'_>) -> (String, Vec<CreateActionRow>) {
        use std::fmt::Write;

        let mut content = String::new();
        let mut buttons = Vec::new();

        // Title
        content.push_str("## ");
        if let Some(team_name) = &self.scrim.team_name {
            write!(content, "{team_name}: ").unwrap();
        }
        write_scrim_meta(&self.scrim, None, &mut content);
        content.push('\n');

        // Body
        match &self.state {
            ScrimState::Looking {
                previous_revoked,
                matches,
            } => {
                if *previous_revoked {
                    content.push_str("Your previous partner picked someone else.\n");
                }
                if matches.is_empty() {
                    content.push_str("No matches found. Try again later\n");
                } else {
                    content.push_str("### Potential matches:\n");
                    for (match_id, (_diff, other)) in matches.iter().enumerate() {
                        write!(content, "{}. ", match_id + 1).unwrap();
                        write_scrim_with_name(other, Some(&self.scrim), true, &mut content);
                        if other.match_id.is_some() {
                            content.push_str(" (picked you)");
                        }
                        content.push('\n');

                        buttons.push(
                            CreateButton::new(format!(
                                "{},accept,{},{}",
                                ctx.id(),
                                self.index,
                                match_id
                            ))
                            .style(ButtonStyle::Success)
                            .label(format!("Accept match {}", match_id + 1)),
                        );
                    }
                }

                buttons.push(
                    CreateButton::new(format!("{},refresh,{}", ctx.id(), self.index))
                        .style(ButtonStyle::Primary)
                        .label("Refresh"),
                );
                buttons.push(
                    CreateButton::new(format!("{},cancel,{}", ctx.id(), self.index))
                        .style(ButtonStyle::Danger)
                        .label("Cancel"),
                );
            }
            ScrimState::Matched(with) => {
                content.push_str("Matched with ");
                write_scrim_with_name(with, Some(&self.scrim), true, &mut content);
                content.push('\n');
                content.push_str("Remember to message them about the details :)");
                buttons.push(
                    CreateButton::new(format!("{},revoke,{}", ctx.id(), self.index))
                        .style(ButtonStyle::Danger)
                        .label("Revoke"),
                );
            }
            ScrimState::Cancelled => {
                content.push_str("Scrim cancelled and removed from matchmaker");
                buttons.push(
                    CreateButton::new(format!("{},restore,{}", ctx.id(), self.index))
                        .style(ButtonStyle::Primary)
                        .label("Restore"),
                );
            }
        }

        if buttons.is_empty() {
            (content, Vec::new())
        } else {
            (
                content,
                buttons
                    .chunks(5)
                    .map(|chunk| CreateActionRow::Buttons(chunk.to_vec()))
                    .collect(),
            )
        }
    }
}

#[poise::command(
    slash_command,
    description_localized("en-US", "List your upcoming scrims"),
    ephemeral
)]
#[tracing::instrument(err, skip(ctx), fields(user = ctx.author().id.get()))]
pub async fn scrims(ctx: Context<'_>) -> Result<(), Error> {
    let mut scrims = db::get_future_scrims(ctx).await?;
    scrims.sort_by_key(|scrim| scrim.time);
    if scrims.is_empty() {
        ctx.reply("You have no upcoming scrims").await?;
        return Ok(());
    }

    let mut msgs: Vec<_> = stream::iter(scrims.into_iter())
        .enumerate()
        .then(|(index, scrim)| ScrimMsg::new(ctx, scrim, index))
        .try_collect()
        .await?;

    let handles: Vec<_> = stream::iter(msgs.iter())
        .then(|msg| async {
            let (content, components) = msg.msg(ctx);
            ctx.send(
                CreateReply::default()
                    .content(content)
                    .components(components),
            )
            .await
        })
        .try_collect()
        .await?;

    let delete_handle = if msgs.len() > 2 {
        Some(
            ctx.send(
                CreateReply::default().components(vec![CreateActionRow::Buttons(vec![
                    CreateButton::new(format!("{},remove_msgs,0", ctx.id()))
                        .style(ButtonStyle::Secondary)
                        .label("Remove messages"),
                ])]),
            )
            .await?,
        )
    } else {
        None
    };

    let ctx_id_str = ctx.id().to_string();
    let mut listener = ComponentInteractionCollector::new(ctx)
        .filter(move |i| i.data.custom_id.starts_with(&ctx_id_str))
        .timeout(std::time::Duration::from_secs(30 * 60))
        .stream();

    while let Some(i) = listener.next().await {
        let mut split = i.data.custom_id.split(',');
        _ = split.next(); // skip the ctx_id
        let action = split.next().unwrap();
        let scrim_id = split.next().unwrap().parse::<usize>().unwrap();
        let scrim = &mut msgs[scrim_id];

        match action {
            "refresh" => {
                scrim.state = ScrimState::Looking {
                    previous_revoked: false,
                    matches: db::find_matches(ctx, &scrim.scrim).await?,
                };
                respond(ctx, i, scrim.msg(ctx)).await?;
            }
            "cancel" => {
                db::cancel_scrim(ctx, scrim.scrim.id).await?;
                scrim.state = ScrimState::Cancelled;
                respond(ctx, i, scrim.msg(ctx)).await?;
            }
            "restore" => {
                db::restore_scrim(ctx, scrim.scrim.id).await?;
                scrim.state = ScrimState::Looking {
                    previous_revoked: false,
                    matches: db::find_matches(ctx, &scrim.scrim).await?,
                };
                respond(ctx, i, scrim.msg(ctx)).await?;
            }
            "revoke" => {
                db::revoke_scrim(ctx, scrim.scrim.id).await?;
                scrim.state = ScrimState::Looking {
                    previous_revoked: false,
                    matches: db::find_matches(ctx, &scrim.scrim).await?,
                };
                respond(ctx, i, scrim.msg(ctx)).await?;
            }
            "accept" => {
                let match_id = split.next().unwrap().parse::<usize>().unwrap();
                let ScrimState::Looking { matches, .. } = &scrim.state else {
                    continue;
                };
                let other = &matches[match_id].1;

                db::match_scrims(ctx, scrim.scrim.id, other.id).await?;
                scrim.state = ScrimState::Matched(other.clone());
                respond(ctx, i, scrim.msg(ctx)).await?;
            }
            "remove_msgs" => {
                for h in handles {
                    h.delete(ctx.into()).await?;
                }
                if let Some(h) = delete_handle {
                    h.delete(ctx.into()).await?;
                }
                return Ok(());
            }
            _ => continue,
        }
    }

    // Timeout
    for h in handles {
        h.delete(ctx.into()).await?;
    }

    Ok(())
}

async fn respond(
    ctx: Context<'_>,
    i: ComponentInteraction,
    (content, components): (String, Vec<CreateActionRow>),
) -> Result<(), serenity::Error> {
    i.create_response(
        ctx,
        CreateInteractionResponse::UpdateMessage(
            CreateInteractionResponseMessage::new()
                .content(content)
                .components(components),
        ),
    )
    .await
}

pub fn write_scrim_with_name(
    lfs: &LookingForScrim,
    other: Option<&LookingForScrim>,
    show_creator: bool,
    content: &mut String,
) {
    use std::fmt::Write;

    if show_creator {
        write!(content, "<@{}> ", lfs.creator_id).unwrap();
        if let Some(team_name) = &lfs.team_name {
            write!(content, "(**{team_name}**) ").unwrap();
        }
    } else if let Some(team_name) = &lfs.team_name {
        write!(content, "**{team_name}** ").unwrap();
    }

    write_scrim_meta(lfs, other, content);
}

pub fn write_scrim_meta(
    lfs: &LookingForScrim,
    other: Option<&LookingForScrim>,
    content: &mut String,
) {
    use std::fmt::Write;

    write!(content, "{:?}/{:?} {}", lfs.region, lfs.platform, lfs.range).unwrap();

    let show_time = if let Some(other) = other {
        lfs.time != other.time
    } else {
        true
    };

    if show_time {
        write!(content, " on <t:{}:F>", lfs.time.timestamp(),).unwrap();
        if lfs.time - Utc::now() < chrono::Duration::days(1) {
            write!(content, " (<t:{}:R>)", lfs.time.timestamp()).unwrap();
        }
    }
}
