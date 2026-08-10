//! 8bit風のオリジナルBGMをループ再生する(ユーザー指示、2026-08-10
//! 「ハリアー風のパロディのBGMにして」への対応)。
//!
//! **正直な開示・著作権上の注意**: セガの実際の『スペースハリアー』楽曲
//! (「MAIN THEME」等)をそのまま再現・転用することは著作権上できないため、
//! それを意図的に避け、"疾走感のあるテンポ・シンセ的な矩形波・上昇/下降
//! を繰り返すフレーズ"という**曲想(ジャンルの雰囲気)のみ**を参考にした
//! 完全に新規のオリジナルメロディをここで作曲している。既存曲の旋律・
//! 拍数・構成の引用はない。
//!
//! 波形は矩形波(square wave)による簡易チップチューン合成
//! (`sample-mario`/`sample-maid`の`audio.rs`と同じ方式)。`rodio`の
//! `SamplesBuffer`にPCMサンプルを直接書き込み、`repeat_infinite()`で
//! 無限ループ再生する。

use rodio::{OutputStream, OutputStreamHandle, Sink, Source};

const SAMPLE_RATE: u32 = 44100;

fn square_wave_note(freq: f32, seconds: f32, volume: f32, samples: &mut Vec<f32>) {
    let n = (SAMPLE_RATE as f32 * seconds) as usize;
    for i in 0..n {
        let t = i as f32 / SAMPLE_RATE as f32;
        let phase = (t * freq).fract();
        let value = if phase < 0.5 { volume } else { -volume };
        let fade_start = n.saturating_sub(SAMPLE_RATE as usize / 200);
        let envelope = if i >= fade_start { (n - i) as f32 / (n - fade_start).max(1) as f32 } else { 1.0 };
        samples.push(value * envelope);
    }
}

/// オリジナルの8bit風・疾走感のあるメロディ(上昇/下降フレーズ)を
/// 1ループ分のPCMサンプル列として生成する。既存楽曲の引用はない。
fn compose_original_melody() -> Vec<f32> {
    const NOTE: f32 = 0.1;
    let notes: &[(f32, f32)] = &[
        (392.00, NOTE), // G4
        (440.00, NOTE), // A4
        (493.88, NOTE), // B4
        (587.33, NOTE), // D5
        (659.25, NOTE), // E5
        (587.33, NOTE), // D5
        (493.88, NOTE), // B4
        (440.00, NOTE), // A4
        (392.00, NOTE), // G4
        (349.23, NOTE), // F4
        (392.00, NOTE), // G4
        (440.00, NOTE * 1.5), // A4
        (523.25, NOTE), // C5
        (587.33, NOTE), // D5
        (523.25, NOTE), // C5
        (440.00, NOTE * 2.0), // A4(フレーズの終止)
    ];
    let mut samples = Vec::new();
    for &(freq, dur) in notes {
        square_wave_note(freq, dur, 0.1, &mut samples);
    }
    samples
}

/// BGMを再生開始する。返り値を呼び出し側で保持し続けている間だけ再生
/// される(dropすると停止する)。再生に失敗した場合(オーディオ出力
/// デバイスが無い等)は`None`を返し、BGM無しで続行できるようにする。
pub fn play_bgm_loop() -> Option<(OutputStream, OutputStreamHandle, Sink)> {
    let (stream, handle) = match OutputStream::try_default() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("BGM再生用のオーディオ出力デバイスを開けませんでした(BGM無しで続行します): {e}");
            return None;
        }
    };
    let sink = match Sink::try_new(&handle) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("BGM再生用のSinkを作成できませんでした(BGM無しで続行します): {e}");
            return None;
        }
    };
    let melody = compose_original_melody();
    let source = rodio::buffer::SamplesBuffer::new(1, SAMPLE_RATE, melody).repeat_infinite();
    sink.append(source);
    Some((stream, handle, sink))
}
