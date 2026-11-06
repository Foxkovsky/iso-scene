# iso-scene для VS Code

Предпросмотр `.scene` и `.model` новым движком iso-scene: изометрия, чертёж и
воксели в одной панели, плюс сборка.

## Установка

```bash
cargo install --path crates/scene-cli
cd editors/vscode && npx @vscode/vsce package
code --install-extension iso-scene-0.2.1.vsix
```

## Команды

- **iso-scene: Preview** — панель с тремя вкладками:
  - **Изометрия** — обычный рендер;
  - **Чертёж** — три ортопроекции: сверху (XY), спереди (XZ), сбоку (YZ);
  - **Воксели** — интерактивный предпросмотр `.vox` (колесо — масштаб,
    перетаскивание — сдвиг).
  Обновляется при сохранении файла.
- **iso-scene: Build** — складывает в `out/`: `<имя>.png`, `<имя>.vox`, а для
  `.scene` ещё `<имя>-tiled/scene.tmj`.
- **iso-scene: Export PNG** — `<имя>.png` рядом с исходником.

Для `.model` рисуется лист из всех объектов файла, для `.scene` — сцена.
Плотность вокселей подбирается автоматически под 128 клеток по длинной оси.

CLI ищется в `PATH`, затем в `~/.cargo/bin/iso-scene`; путь задаётся
настройкой `isoScene.executable`.
