# sample-harrier

スペースハリアー風、疑似3Dスケーリングスプライトのショートゲームプロトタイプ。

[`open-directx`](https://github.com/aon-co-jp/open-directx) の
DXBC→SPIR-V翻訳済みスプライトパイプライン
(`directx-shader-translate` / `directx-graphics-vulkan`)を
Cargoの依存(GitHubの`open-directx`)として再利用し、実ウィンドウ+Vulkanスワップチェーンで
動作する。

## 現在のスコープ(Phase 0)

自機の照準(黄)を上下左右に動かし、奥から迫ってくる敵(赤、`enemy_depth`
に応じて拡大)を、十分近づいたタイミングで重ねてヒットさせる——1体の
疑似3Dスケーリングスプライトのみ。地面のスクロール・複数の敵・得点・
ステージ進行は未実装。

## 実行方法

```bash
cargo run --release
```

矢印キー/WASDで照準移動、Escapeで終了。

## ディレクトリ構成

`open-directx` はGitHubから自動取得されます(隣にcloneする必要はありません)。

## 注意 / Notice

セガ(SEGA)の「スペースハリアー」とは無関係の、独自実装の小さなプロトタイプです。キャラクター・音・画像は一切使っていません。

This is an independent prototype unrelated to SEGA's Space Harrier. It uses none of its characters, sounds or images.

## ライセンス / License

MIT(`LICENSE`参照)。 / MIT (see `LICENSE`).

## 配布物 / Releases

Windows (x86_64) のビルド済みバイナリは Releases から。Vulkan対応のGPUとドライバが必要です。他のOSは `cargo run --release`(`open-directx` はGitHubから自動取得されます)。 / Prebuilt Windows (x86_64) binary on the Releases page; a Vulkan-capable GPU and driver are required. Other OSes: `cargo run --release` (`open-directx` is fetched from GitHub automatically).
