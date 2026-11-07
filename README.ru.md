# iso-scene

[![CI](https://github.com/Foxkovsky/iso-scene/actions/workflows/ci.yml/badge.svg)](https://github.com/Foxkovsky/iso-scene/actions/workflows/ci.yml)
![Rust 1.85+](https://img.shields.io/badge/rust-1.85%2B-orange)
![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)

**Компилятор маленького C-подобного языка в пиксель-арт ассеты для игр.**

Опиши предмет один раз простыми телами. iso-scene отрисует его в изометрии
2:1, в ортогональных видах сверху, спереди и сбоку, в SVG, соберёт модель
MagicaVoxel, лист спрайтов с анимациями или готовый к правке уровень Tiled.

```
.model ─┐                          ┌─▶ PNG / SVG        изометрия 2:1, сверху, спереди, сбоку
        ├─▶ разбор ─▶ вычисление ─▶├─▶ VOX              MagicaVoxel, с разбиением на чанки
.scene ─┘   (с бюджетом fuel)      ├─▶ листы спрайтов   JSON + тайлсет Tiled + атлас Phaser/PixiJS
.walls ────────────────────────────┴─▶ карта Tiled      пол, стены, тени, объекты, коллизии
```

> **Статус: до 1.0.** Язык описан (черновик v1) и реализован полностью;
> синтаксис ещё может измениться до 1.0, ломающие изменения — с конвертером.

## Содержание

- [Возможности](#возможности)
- [Установка](#установка)
- [Первая сцена](#первая-сцена)
- [Язык коротко](#язык-коротко)
- [Что можно получить](#что-можно-получить)
- [Редакторы и агенты](#редакторы-и-агенты)
- [Гарантии](#гарантии)
- [Документация](#документация)
- [Разработка](#разработка)
- [Устройство репозитория](#устройство-репозитория)
- [Планы](#планы)
- [Лицензия](#лицензия)

## Возможности

- **Одна модель — все виды.** Изометрия, сверху, спереди и сбоку из одной
  геометрии; попиксельная глубина, солнце и рассеянный свет, материалы в TOML
  с процедурными текстурами.
- **Экспорт для игр.** Листы спрайтов (1–16 направлений, строки анимации,
  якоря, отступы, страницы), `sheet.json` с JSON Schema, тайлсеты Tiled,
  атласы Phaser/PixiJS, модели MagicaVoxel.
- **Уровни Tiled.** Пол, стены с автотайлингом на 16 масок и Wang-набором,
  контактные тени, объекты по глубине — ровно там, где их рисует Tiled, следы
  объектов и предупреждения о коллизиях, расширение Tiled для проверки при
  правке.
- **Анимация в языке.** `animation open (t) frames 7 duration 100;` и любые
  выражения от `t`: двери поворачиваются (`turn 90 * t`), ящики выезжают.
- **Быстро.** Параллельный растеризатор полосами, инкрементальные кадры
  анимации, бенчмарки criterion на каждом горячем пути. Класс из 40 предметов
  рендерится примерно за 25 мс.
- **Безопасно на любом вводе.** Без `unsafe`, без паник на пользовательском
  вводе, лимиты проверяются до выделения памяти, вычисление ограничено fuel,
  `include` не выходит за пределы проекта.
- **Редакторы и агенты.** Языковой сервер, MCP-сервер, который отдаёт агенту
  готовую картинку, и расширение VS Code с живым предпросмотром.

## Установка

```bash
git clone https://github.com/Foxkovsky/iso-scene
cd iso-scene
cargo install --path crates/scene-cli     # ставит команду `iso-scene`
```

Нужен Rust 1.85 или новее. Python, Node и GPU не нужны.

## Первая сцена

`crate.scene`:

```c
version 1;

object crate (1, 1) {
    box at (0, 0, 0) size (w, d, 1) material wood;
}

render iso;
scale 32;

place crate at (0, 0, 0);
place crate at (1.2, 0, 0) rot 90;
```

```bash
iso-scene render crate.scene -o crate.png          # PNG
iso-scene render crate.scene -o crate.svg          # SVG
iso-scene render crate.scene --view top -o top.png # ортогональный вид
```

Посмотри на картинку, поправь одно и отрисуй снова.

## Язык коротко

Модели (`.model`) описывают переиспользуемые объекты; сцены (`.scene`)
подключают модели, расставляют их и выбирают проекцию. Все длины — в
**блоках**, клетках карты; `scale` (пикселей на блок) задаёт сцена, а не
модель.

```c
// desk.model
version 1;
material oak = "materials/oak.toml";
material steel = "materials/steel.toml";

object desk (5, 2) {                       // след w × d в блоках
    let leg = 0.16;
    box at (0.16, 0.16, 0) size (leg, leg, 2.8) material steel;
    mirror x;                              // четыре ножки из одной
    mirror y;
    box at (0, 0, 2.8) size (w, d, 0.16) material oak;
}
```

```c
// door.scene
version 1;
include "desk.model";

object door (2, 0.2) {
    box at (0, 0, 0) size (w, d, 4) material oak;
}

render iso;
scale 24;

animation open (t) frames 7 duration 100 once;

for i in 0..3 {
    place desk at (i * 6, 0, 0) rot 0 anchor back;
}
place door at (-3, 2, 0) turn 80 * t;      // петля — в локальной точке (0, 0) двери
```

Тела: `box`, `cyl`, `sphere`, `slantbox`, `slantcyl`, `ramp`. Ещё `let` /
`var`, `for`, `if`, `mirror`, типовые детали через `use` и `merge` для
бесшовных сварных поверхностей. `iso-scene help <тема>` печатает нужный
раздел спецификации.

## Что можно получить

| Команда | Результат |
|---|---|
| `render scene -o out.png` / `out.svg` | PNG в изометрии или ортогонально, SVG изометрии |
| `voxel scene -o out.vox` | модель MagicaVoxel, больше 256 по оси — чанками |
| `sprites a.model b.scene --views 8 --atlas` | лист спрайтов, `sheet.json`, тайлсеты Tiled, атлас Phaser/PixiJS |
| `animate scene` | каждая `animation` — лента кадров, анимированный тайл Tiled и карта |
| `tiled scene --walls room.walls` | изометрическая карта Tiled: пол, стены, тени, объекты, коллизии |
| `check` / `eval` | диагностика / вычисленный граф сцены |
| `help [тема]` / `limits` | раздел спецификации / все лимиты с кодами ошибок |

Диагностика выглядит так:
``room.scene:4:1: error[E0301]: unknown object `sofa` ``; коды перечислены в спецификации.

## Редакторы и агенты

**VS Code.** `editors/vscode`: живой предпросмотр (изометрия, чертёж,
воксели), ошибки в Problems при наборе, подсказки, переход к объявлению,
структура файла, автодополнение, сборка и лист спрайтов.

```bash
cd editors/vscode && npx @vscode/vsce package
code --install-extension iso-scene-0.4.0.vsix
```

**Любой редактор с LSP.** `iso-scene lsp` работает по Language Server
Protocol через stdio (настройки Neovim и Helix — в
[`docs/servers.md`](docs/servers.md)).

**ИИ-агенты (MCP).** `iso-scene mcp` даёт инструменты `check`, `render`
(возвращает картинку), `export`, `help` и `limits`. Для Claude Code:

```bash
claude mcp add iso-scene -- iso-scene mcp
```

**Tiled.** `editors/tiled/iso-scene.js` проверяет коллизии по текущим
позициям объектов (Map → Check iso-scene Collisions).

## Гарантии

- **Детерминированный вывод.** Одинаковый вход даёт побайтно одинаковые PNG,
  SVG, VOX и JSON на любой машине и при любом числе потоков.
- **Диагностика, а не падение.** Испорченный или враждебный ввод (глубокая
  вложенность, огромные циклы, гигантские сетки) заканчивается диагностикой с
  позицией; лимиты выводит `iso-scene limits`, и тест сверяет их со
  спецификацией.
- **Никаких сюрпризов на диске.** Пути `include` и `material`, как и все
  пути MCP-сервера, не выходят за корень проекта.

## Документация

| Документ | О чём |
|---|---|
| [`docs/spec.md`](docs/spec.md) | семантика языка, лимиты, диагностика (нормативно) |
| [`docs/grammar.ebnf`](docs/grammar.ebnf) | грамматика (нормативно) |
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | конвейер, крейты, детерминизм, производительность |
| [`docs/backends.md`](docs/backends.md) | рендереры, экспорт в Tiled, листы спрайтов, анимация |
| [`docs/walls.md`](docs/walls.md) | формат `.walls`, слои стен и тени в Tiled |
| [`docs/projections.md`](docs/projections.md) | формулы проекций и их проверки |
| [`docs/servers.md`](docs/servers.md) | языковой сервер и MCP-сервер |
| [`docs/sprite-sheet.schema.json`](docs/sprite-sheet.schema.json) | схема `sheet.json` |

Примеры — в [`examples/`](examples): стул, стол, класс в изометрии и сверху,
анимированная дверь.

## Разработка

```bash
make fmt          # форматирование
make ci           # fmt-check, clippy -D warnings, тесты, документация
make bench        # бенчмарки criterion
make vscode-test  # тесты расширения VS Code в Node
make vscode-it    # расширение внутри VS Code (нужны code и xvfb-run)
```

Прочитай [`CONTRIBUTING.md`](CONTRIBUTING.md) и [`AGENTS.md`](AGENTS.md).
Изменение синтаксиса или семантики обновляет `docs/spec.md` и
`docs/grammar.ebnf` в том же коммите; изменение, способное повлиять на
скорость, приводит цифры бенчмарков.

## Устройство репозитория

```
crates/scene-syntax/          лексер, парсер, AST, диагностика
crates/scene-core/            вычисление, граф сцены, лимиты
crates/scene-render/          растеризатор, изображения, материалы
crates/scene-backend-iso/     изометрия 2:1 и SVG
crates/scene-backend-ortho/   виды сверху, спереди, сбоку
crates/scene-backend-voxel/   вокселизация и запись VOX
crates/scene-walls/           формат .walls, автотайлинг, тени
crates/scene-export-sprites/  листы спрайтов и анимации
crates/scene-export-tiled/    карты Tiled, следы, коллизии
crates/scene-server/          темы справки, лимиты, серверы LSP и MCP
crates/scene-cli/             команда `iso-scene`
editors/vscode/               расширение VS Code
editors/tiled/                расширение Tiled
docs/                         спецификация и руководства
examples/                     модели и сцены
```

## Планы

| Готово | Дальше |
|---|---|
| язык, вычислитель, изометрия, ортогональные виды, SVG, воксели | форматтер |
| листы спрайтов, анимации в языке | браузерный навигатор с WASM-рендером на устройстве |
| экспорт в Tiled со стенами, тенями и коллизиями | параметрические объекты (переопределение `w`, `d`) |
| языковой сервер, MCP-сервер, расширение VS Code | замороженная грамматика и 1.0 |

## Лицензия

Код: **MIT OR Apache-2.0**. Ассеты и примеры: **CC BY 4.0**.
См. [`LICENSE-MIT`](LICENSE-MIT), [`LICENSE-APACHE`](LICENSE-APACHE),
[`LICENSE-ASSETS`](LICENSE-ASSETS).

English version: [`README.md`](README.md).
