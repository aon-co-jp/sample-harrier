# sample-harrier

スペースハリアー風、疑似3Dスケーリングスプライトのショートゲームプロトタイプ。

[`open-directx`](https://github.com/aon-co-jp/open-directx) の
DXBC→SPIR-V翻訳済みスプライトパイプライン
(`directx-shader-translate` / `directx-graphics-vulkan`)を
Cargo pathの依存として再利用し、実ウィンドウ+Vulkanスワップチェーンで
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

`open-directx` を隣に(`../open-directx`)cloneしている前提の
Cargo path依存になっている。
