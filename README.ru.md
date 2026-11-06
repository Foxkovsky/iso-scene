# iso-scene

**Компилятор 3D-моделей и сцен в пиксель-арт.**

Опиши предмет один раз простыми телами — и получи его в изометрии 2:1,
в ортогональной проекции сверху или сбоку, в вокселях или как уровень Tiled.

> Статус: **pre-alpha**. Спецификация языка фиксируется, реализация на Rust —
> следующий этап. Ничего пока не стабильно.

## Зачем

Рисовать каждый вид предмета руками — не масштабируется. 3D-модель даёт все
виды сразу:

```
.model  ──▶  геометрия + материалы + циклы
                  │
                  ├── изометрия 2:1  ──▶ PNG / SVG
                  ├── сверху / сбоку  ──▶ PNG / SVG
                  ├── воксели         ──▶ VOX
                  └── Tiled           ──▶ TMJ + тайлсеты
.scene  ──▶  расстановка, свет, масштаб, экспорт
```

## Принципы

- **Одна единица — блок.** Блок — клетка карты. Физический размер (10 см,
  25 см, 1 м) — соглашение проекта, а не свойство языка.
- **Модель не зависит от разрешения.** `scale` (пикселей на блок) задаётся
  в сцене или команде рендера, но не в модели.
- **C-подобный синтаксис.** `;`, `{}`, `//` и `/* */`; переводы строк и
  отступы не важны.
- **Вычисление с бюджетом.** Циклы разрешены, но каждый шаг тратит fuel.
  Плохая сцена выдаёт диагностику, а не вешает редактор.
- **Модели и сцены разделены.** `.model` — переиспользуемые объекты,
  `.scene` — расстановка, свет, проекция и экспорт.

## Быстрый старт

```bash
cargo run -p scene-cli -- --version
cargo run -p scene-cli -- check examples/chair.model
cargo run -p scene-cli -- eval examples/chair.model
cargo run -p scene-cli -- eval examples/classroom.scene
cargo run -p scene-cli -- render examples/classroom.scene -o out/classroom.png
# векторный экспорт (только iso): -o out/classroom.svg
cargo run -p scene-cli -- render examples/classroom_top.scene -o out/classroom_top.png
cargo run -p scene-cli -- voxel examples/classroom.scene --density 7 -o out/classroom.vox
cargo run -p scene-cli -- tiled examples/classroom.scene -o out/tiled
```

Лексер, парсер, вычислитель, изометрический и ортогональные бэкенды,
материалы, воксельный экспорт и экспорт в Tiled с отдельным слоем стен
реализованы. Нормативные документы:

- [`docs/spec.md`](docs/spec.md) — семантика языка;
- [`docs/grammar.ebnf`](docs/grammar.ebnf) — грамматика;
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — устройство и крейты;
- [`docs/projections.md`](docs/projections.md) — формулы проекций и их проверки;
- [`docs/walls.md`](docs/walls.md) — формат `.walls`, 16 масок и слой стен в Tiled.

Примеры: [`examples/chair.model`](examples/chair.model),
[`examples/classroom.scene`](examples/classroom.scene).

Результаты рендера и другие сгенерированные бинарники не коммитятся; их
можно пересобрать командами выше.

## Лицензия

Код: **MIT OR Apache-2.0**. Ассеты и примеры: **CC BY 4.0**.
См. `LICENSE-MIT`, `LICENSE-APACHE`, `LICENSE-ASSETS`.
