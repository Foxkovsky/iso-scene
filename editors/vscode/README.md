# iso-scene для VS Code

Предпросмотр `.scene` и `.model` файлов новым движком iso-scene.

## Установка

1. Собери CLI:

   ```bash
   cargo install --path crates/scene-cli
   ```

2. Поставь расширение:

   ```bash
   code --install-extension iso-scene-0.2.0.vsix
   ```

## Команды

- **iso-scene: Preview** — открыть предпросмотр рядом с редактором; обновляется
  при сохранении файла.
- **iso-scene: Export PNG** — сохранить `<имя>.png` рядом с исходником.
- **iso-scene: Export Tiled map** — сохранить `<имя>-tiled/scene.tmj`.

Для `.model` рисуется лист из всех объектов файла, для `.scene` — сцена.

CLI ищется в `PATH`, затем в `~/.cargo/bin/iso-scene`. Путь можно задать
настройкой `isoScene.executable`.
