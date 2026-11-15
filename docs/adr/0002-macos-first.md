# 最初の OS は macOS にする

日付は 2026-11-15 です。決定者は Hayato です。状態は置き換え済みです。後継は [最初の OS は Linux にする](0004-linux-first.md) です。ここから下は、置き換えられる前の決定です。

最初に動かす OS は macOS です。次は Linux (Omarchy) で、 Windows はその後です。 Windows を動かす作業は、 `docs/board.md` の TODO にあります。

## 理由

macOS の GPUI には、コピーなしで表示する経路があります。先行例の [bokuweb/gpui-cef](https://github.com/bokuweb/gpui-cef) も、 macOS で CEF を GPUI に表示しています。

Linux で画素をコピーする理由は、 [CEF をページの中身にする](0001-cef.md) にあります。
