use poise::serenity_prelude::{
    ButtonStyle, Colour, CreateActionRow, CreateAttachment, CreateButton, CreateEmbed,
    CreateEmbedFooter, CreateInteractionResponseMessage,
};

use crate::error::EMBED_COLOR;

pub fn embed(title: impl Into<String>, description: impl Into<String>) -> CreateEmbed {
    CreateEmbed::new()
        .title(title)
        .description(description)
        .color(Colour::new(EMBED_COLOR))
        .footer(CreateEmbedFooter::new("Nooora"))
}

pub fn button(id: impl Into<String>, label: impl Into<String>, style: ButtonStyle) -> CreateButton {
    CreateButton::new(id).label(label).style(style)
}

pub fn link_button(url: impl Into<String>, label: impl Into<String>) -> CreateButton {
    CreateButton::new_link(url).label(label)
}

pub fn row(buttons: Vec<CreateButton>) -> CreateActionRow {
    CreateActionRow::Buttons(buttons)
}

pub fn chunk_buttons(buttons: Vec<CreateButton>) -> Vec<CreateActionRow> {
    buttons
        .chunks(5)
        .map(|chunk| CreateActionRow::Buttons(chunk.to_vec()))
        .collect()
}

pub struct View {
    pub content: String,
    pub embed: CreateEmbed,
    pub components: Vec<CreateActionRow>,
    pub file: Option<(Vec<u8>, String)>,
}

impl View {
    pub fn new(embed: CreateEmbed) -> Self {
        Self {
            content: String::new(),
            embed,
            components: Vec::new(),
            file: None,
        }
    }

    pub fn content(mut self, content: impl Into<String>) -> Self {
        self.content = content.into();
        self
    }

    pub fn components(mut self, components: Vec<CreateActionRow>) -> Self {
        self.components = components;
        self
    }

    pub fn file(mut self, bytes: Vec<u8>, name: impl Into<String>) -> Self {
        self.file = Some((bytes, name.into()));
        self
    }

    pub fn into_reply(self) -> poise::CreateReply {
        let mut reply = poise::CreateReply::default()
            .content(self.content)
            .embed(self.embed)
            .components(self.components);
        if let Some((bytes, name)) = self.file {
            reply = reply.attachment(CreateAttachment::bytes(bytes, name));
        }
        reply
    }

    pub fn into_update(self) -> CreateInteractionResponseMessage {
        let mut message = CreateInteractionResponseMessage::new()
            .content(self.content)
            .embed(self.embed)
            .components(self.components);
        if let Some((bytes, name)) = self.file {
            message = message.add_file(CreateAttachment::bytes(bytes, name));
        }
        message
    }
}

pub fn trunc(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}
