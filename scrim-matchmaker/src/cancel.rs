use std::{time::Duration, vec};

use poise::{
    serenity_prelude::{
        self as serenity, futures::StreamExt, ButtonStyle, ComponentInteractionDataKind,
        CreateActionRow, CreateButton, CreateInteractionResponseMessage, CreateSelectMenu,
        CreateSelectMenuKind, CreateSelectMenuOption,
    },
    CreateReply,
};

use crate::{db, Context, Error};

#[poise::command(
    slash_command,
    description_localized("en-US", "Cancel scrims. This removes them from the matchmaker."),
    ephemeral
)]
#[tracing::instrument(err, skip(ctx), fields(user = ctx.author().id.get()))]
pub async fn cancel(ctx: Context<'_>) -> Result<(), Error> {
    let scrims = db::get_future_scrims(ctx).await?;
    let tz = db::get_timezone(ctx).await?;

    let cancel_select = CreateSelectMenu::new(
        "select",
        CreateSelectMenuKind::String {
            options: scrims
                .iter()
                .enumerate()
                .map(|(i, scrim)| {
                    use std::fmt::Write;
                    let mut info = String::new();
                    if let Some(team_name) = &scrim.team_name {
                        write!(&mut info, "{}: ", team_name).unwrap();
                    }
                    write!(
                        &mut info,
                        "{:?}/{:?} {} on {}",
                        scrim.region,
                        scrim.platform,
                        scrim.range,
                        scrim.time.with_timezone(&tz).format("%A, %B %d, %H:%M %Z")
                    )
                    .unwrap();
                    CreateSelectMenuOption::new(info, i.to_string())
                })
                .collect(),
        },
    )
    .max_values(scrims.len() as u8)
    .min_values(0);

    let msg_handle = ctx
        .send(
            CreateReply::default()
                .content("Select the scrims you want to cancel:")
                .components(vec![
                    CreateActionRow::SelectMenu(cancel_select),
                    CreateActionRow::Buttons(vec![CreateButton::new("confirm")
                        .style(ButtonStyle::Danger)
                        .label("Confirm")]),
                ]),
        )
        .await?;

    let mut listener = msg_handle
        .message()
        .await?
        .await_component_interaction(ctx)
        .timeout(Duration::from_secs(60 * 60))
        .stream();

    let mut to_cancel = Vec::new();

    while let Some(i) = listener.next().await {
        match &i.data.kind {
            ComponentInteractionDataKind::StringSelect { values } => {
                to_cancel.clone_from(values);
                i.create_response(ctx, serenity::CreateInteractionResponse::Acknowledge)
                    .await?;
            }
            ComponentInteractionDataKind::Button if i.data.custom_id == "confirm" => {
                if to_cancel.is_empty() {
                    i.create_response(
                        ctx,
                        serenity::CreateInteractionResponse::UpdateMessage(
                            CreateInteractionResponseMessage::new()
                                .content("No scrims selected.")
                                .components(vec![]),
                        ),
                    )
                    .await?;
                    return Ok(()); // Aborted
                } else {
                    for i in to_cancel {
                        let i = i.parse::<usize>().unwrap();
                        db::cancel_scrim(ctx, scrims[i].id).await?;
                    }
                    i.create_response(
                        ctx,
                        serenity::CreateInteractionResponse::UpdateMessage(
                            CreateInteractionResponseMessage::new()
                                .content("Scrim(s) cancelled.")
                                .components(vec![]),
                        ),
                    )
                    .await?;
                }
                return Ok(()); // Confirmed
            }
            _ => continue,
        }
    }

    // Timeout
    msg_handle.delete(ctx.into()).await?;
    Ok(())
}
