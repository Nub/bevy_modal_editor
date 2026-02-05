use std::sync::Arc;

use bevy::prelude::*;
use bevy_editor_game::GameState;

use crate::levels::LevelCompleteEvent;
use crate::marble::MarbleJumpedEvent;

/// Holds pre-generated sound effect handles.
#[derive(Resource)]
pub struct SoundAssets {
    jump: Handle<AudioSource>,
    goal: Handle<AudioSource>,
}

pub struct SoundEffectsPlugin;

impl Plugin for SoundEffectsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_sounds).add_systems(
            Update,
            (play_jump_sound, play_goal_sound).run_if(in_state(GameState::Playing)),
        );
    }
}

/// Generate procedural WAV sounds and store handles.
fn setup_sounds(mut commands: Commands, mut audio_assets: ResMut<Assets<AudioSource>>) {
    let jump = audio_assets.add(AudioSource {
        bytes: Arc::from(generate_jump_wav()),
    });
    let goal = audio_assets.add(AudioSource {
        bytes: Arc::from(generate_goal_wav()),
    });
    commands.insert_resource(SoundAssets { jump, goal });
}

fn play_jump_sound(
    mut events: MessageReader<MarbleJumpedEvent>,
    mut commands: Commands,
    sounds: Res<SoundAssets>,
) {
    for _ in events.read() {
        commands.spawn((
            AudioPlayer(sounds.jump.clone()),
            PlaybackSettings::DESPAWN,
        ));
    }
}

fn play_goal_sound(
    mut events: MessageReader<LevelCompleteEvent>,
    mut commands: Commands,
    sounds: Res<SoundAssets>,
) {
    for _ in events.read() {
        commands.spawn((
            AudioPlayer(sounds.goal.clone()),
            PlaybackSettings::DESPAWN,
        ));
    }
}

// ---------------------------------------------------------------------------
// Procedural WAV generation
// ---------------------------------------------------------------------------

const SAMPLE_RATE: u32 = 44100;

/// Generate a short rising chirp (~0.15s): sine sweep 200Hz → 600Hz.
fn generate_jump_wav() -> Vec<u8> {
    let duration_secs = 0.15;
    let num_samples = (SAMPLE_RATE as f64 * duration_secs) as usize;
    let mut samples = Vec::with_capacity(num_samples);

    for i in 0..num_samples {
        let t = i as f64 / SAMPLE_RATE as f64;
        let progress = t / duration_secs;

        // Frequency sweep from 200 to 600 Hz
        let freq = 200.0 + 400.0 * progress;
        // Phase accumulation for smooth sweep
        let phase = 2.0 * std::f64::consts::PI * (200.0 * t + 200.0 * t * progress);
        let amplitude = 0.6 * (1.0 - progress); // Fade out
        let sample = (amplitude * phase.sin()) as f32;
        samples.push(sample);
        let _ = freq; // suppress unused warning
    }

    encode_wav_mono(&samples)
}

/// Generate a triumphant chord (~0.5s): C major (C4 + E4 + G4 + C5).
fn generate_goal_wav() -> Vec<u8> {
    let duration_secs = 0.5;
    let num_samples = (SAMPLE_RATE as f64 * duration_secs) as usize;
    let mut samples = Vec::with_capacity(num_samples);

    let freqs = [261.63, 329.63, 392.00, 523.25]; // C4, E4, G4, C5

    for i in 0..num_samples {
        let t = i as f64 / SAMPLE_RATE as f64;
        let progress = t / duration_secs;

        // Attack-decay envelope
        let envelope = if progress < 0.05 {
            progress / 0.05 // Quick attack
        } else {
            (1.0 - (progress - 0.05) / 0.95).max(0.0) // Decay
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

/// Encode f32 samples as a 16-bit mono WAV file.
fn encode_wav_mono(samples: &[f32]) -> Vec<u8> {
    let num_samples = samples.len() as u32;
    let bytes_per_sample = 2u16; // 16-bit
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
    buf.extend_from_slice(&16u32.to_le_bytes()); // chunk size
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM format
    buf.extend_from_slice(&num_channels.to_le_bytes());
    buf.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    let byte_rate = SAMPLE_RATE * num_channels as u32 * bytes_per_sample as u32;
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    let block_align = num_channels * bytes_per_sample;
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&(bytes_per_sample * 8).to_le_bytes()); // bits per sample

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
