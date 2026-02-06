//! Procedural sound effects: coin collect chime, all-collected fanfare.

use std::sync::Arc;

use bevy::prelude::*;
use bevy_editor_game::GameState;

use crate::coins::{AllCoinsCollectedEvent, CoinCollectedEvent};

#[derive(Resource)]
struct SoundAssets {
    coin_collect: Handle<AudioSource>,
    all_collected: Handle<AudioSource>,
}

pub struct SoundEffectsPlugin;

impl Plugin for SoundEffectsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_sounds).add_systems(
            Update,
            (play_coin_sound, play_complete_sound).run_if(in_state(GameState::Playing)),
        );
    }
}

fn setup_sounds(mut commands: Commands, mut audio_assets: ResMut<Assets<AudioSource>>) {
    let coin_collect = audio_assets.add(AudioSource {
        bytes: Arc::from(generate_coin_wav()),
    });
    let all_collected = audio_assets.add(AudioSource {
        bytes: Arc::from(generate_fanfare_wav()),
    });
    commands.insert_resource(SoundAssets {
        coin_collect,
        all_collected,
    });
}

fn play_coin_sound(
    mut events: MessageReader<CoinCollectedEvent>,
    mut commands: Commands,
    sounds: Res<SoundAssets>,
) {
    for _ in events.read() {
        commands.spawn((
            AudioPlayer(sounds.coin_collect.clone()),
            PlaybackSettings::DESPAWN,
        ));
    }
}

fn play_complete_sound(
    mut events: MessageReader<AllCoinsCollectedEvent>,
    mut commands: Commands,
    sounds: Res<SoundAssets>,
) {
    for _ in events.read() {
        commands.spawn((
            AudioPlayer(sounds.all_collected.clone()),
            PlaybackSettings::DESPAWN,
        ));
    }
}

// ---------------------------------------------------------------------------
// Procedural WAV generation
// ---------------------------------------------------------------------------

const SAMPLE_RATE: u32 = 44100;

/// Bright rising chime for coin collection (100ms, sine sweep 400->800Hz).
fn generate_coin_wav() -> Vec<u8> {
    let duration_secs = 0.1;
    let num_samples = (SAMPLE_RATE as f64 * duration_secs) as usize;
    let mut samples = Vec::with_capacity(num_samples);

    for i in 0..num_samples {
        let t = i as f64 / SAMPLE_RATE as f64;
        let progress = t / duration_secs;

        // Frequency sweep from 400 to 800 Hz
        let freq_start = 400.0;
        let freq_end = 800.0;
        let freq = freq_start + (freq_end - freq_start) * progress;
        let phase = 2.0 * std::f64::consts::PI * (freq_start * t + (freq_end - freq_start) * 0.5 * t * progress);
        let amplitude = 0.6 * (1.0 - progress); // Fade out
        let sample = (amplitude * phase.sin()) as f32;
        samples.push(sample);
        let _ = freq;
    }

    encode_wav_mono(&samples)
}

/// Triumphant arpeggio for all coins collected (500ms, C5+E5+G5+C6).
fn generate_fanfare_wav() -> Vec<u8> {
    let duration_secs = 0.5;
    let num_samples = (SAMPLE_RATE as f64 * duration_secs) as usize;
    let mut samples = Vec::with_capacity(num_samples);

    // C5, E5, G5, C6
    let freqs = [523.25, 659.25, 783.99, 1046.50];

    for i in 0..num_samples {
        let t = i as f64 / SAMPLE_RATE as f64;
        let progress = t / duration_secs;

        // ADSR envelope
        let envelope = if progress < 0.05 {
            progress / 0.05
        } else {
            (1.0 - (progress - 0.05) / 0.95).max(0.0)
        };

        let mut sample = 0.0_f64;
        for &freq in &freqs {
            let phase = 2.0 * std::f64::consts::PI * freq * t;
            sample += phase.sin();
        }
        sample = sample / freqs.len() as f64 * 0.7 * envelope;
        samples.push(sample as f32);
    }

    encode_wav_mono(&samples)
}

fn encode_wav_mono(samples: &[f32]) -> Vec<u8> {
    let num_samples = samples.len() as u32;
    let bytes_per_sample = 2u16;
    let num_channels = 1u16;
    let data_size = num_samples * bytes_per_sample as u32;
    let file_size = 36 + data_size;

    let mut buf = Vec::with_capacity(file_size as usize + 8);

    // RIFF header
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&file_size.to_le_bytes());
    buf.extend_from_slice(b"WAVE");

    // fmt chunk
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
    buf.extend_from_slice(&num_channels.to_le_bytes());
    buf.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    let byte_rate = SAMPLE_RATE * num_channels as u32 * bytes_per_sample as u32;
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    let block_align = num_channels * bytes_per_sample;
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&(bytes_per_sample * 8).to_le_bytes());

    // data chunk
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_size.to_le_bytes());
    for &sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        let int_val = (clamped * i16::MAX as f32) as i16;
        buf.extend_from_slice(&int_val.to_le_bytes());
    }

    buf
}
