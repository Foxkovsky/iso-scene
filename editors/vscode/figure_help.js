'use strict';
// Подсказки для нового синтаксиса iso-scene.
module.exports = {
    box: {
        signature: 'box at (x, y, z) size (w, d, h) [material]',
        description: 'Прямоугольный блок: дальний нижний угол и размеры. Размеры — в блоках.',
        snippet: 'box at (${1:0}, ${2:0}, ${3:0}) size (${4:1}, ${5:1}, ${6:1}) material ${7:wood}'
    },
    'box from': {
        signature: 'box from (x1, y1, z1) to (x2, y2, z2) [material]',
        description: 'Блок по двум противоположным углам; порядок точек любой.',
        snippet: 'box from (${1:0}, ${2:0}, ${3:0}) to (${4:1}, ${5:1}, ${6:1}) material ${7:wood}'
    },
    cyl: {
        signature: 'cyl at (x, y, z) radius r height h [material]',
        description: 'Вертикальный цилиндр: центр нижнего основания, радиус и высота.',
        snippet: 'cyl at (${1:0}, ${2:0}, ${3:0}) radius ${4:0.5} height ${5:1} material ${6:metal}'
    },
    sphere: {
        signature: 'sphere at (x, y, z) radius r [material]',
        description: 'Сфера: центр и радиус.',
        snippet: 'sphere at (${1:0}, ${2:0}, ${3:0}) radius ${4:0.5} material ${5:glass}'
    },
    slantbox: {
        signature: 'slantbox at (x, y, z) size (w, d, h) offset (dx, dy) [material]',
        description: 'Наклонный блок: верхняя грань сдвинута на offset.',
        snippet: 'slantbox at (${1:0}, ${2:0}, ${3:0}) size (${4:1}, ${5:1}, ${6:1}) offset (${7:0.2}, ${8:0.1}) material ${9:wood}'
    },
    slantcyl: {
        signature: 'slantcyl at (x, y, z) radius r height h offset (dx, dy) [material]',
        description: 'Наклонный цилиндр: верхнее основание сдвинуто на offset.',
        snippet: 'slantcyl at (${1:0}, ${2:0}, ${3:0}) radius ${4:0.5} height ${5:1} offset (${6:0.2}, ${7:0}) material ${8:metal}'
    },
    ramp: {
        signature: 'ramp at (x, y, z) size (w, d) rise (h0, h1) [material]',
        description: 'Пандус: высота в начале и конце по оси X.',
        snippet: 'ramp at (${1:0}, ${2:0}, ${3:0}) size (${4:2}, ${5:1}) rise (${6:0}, ${7:1}) material ${8:wood}'
    },
    object: {
        signature: 'object name (w, d) [origin back|center|front] { ... }',
        description: 'Объект: план w × d и тело. origin center — локальный (0,0) в центре следа.',
        snippet: 'object ${1:name} (${2:1}, ${3:1}) {\n    ${0}\n}'
    },
    use: {
        signature: 'use name [at (x, y, z)] [rot r];',
        description: 'Вставить другой объект. Только внутри объекта; rot кратен 90.',
        snippet: 'use ${1:name} at (${2:0}, ${3:0}, ${4:0}) rot ${5:0};'
    },
    merge: {
        signature: 'merge { ... }',
        description: 'Объединить детали: убрать внутренние грани и швы между ними. Снаружи блока швы остаются.',
        snippet: 'merge {\n    ${0}\n}'
    },
    place: {
        signature: 'place name at (x, y, z) [rot r] [anchor back|center|front|left|right|cell];',
        description: 'Поставить объект. anchor cell — клетка под спрайтом для тайлового слоя.',
        snippet: 'place ${1:name} at (${2:0}, ${3:0}, ${4:0}) anchor ${5:cell};'
    }
};
