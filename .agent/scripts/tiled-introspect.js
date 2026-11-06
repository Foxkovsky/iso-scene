const map = tiled.open("/home/foxkovsky/Рабочий стол/iso-tiled-demo/tiled/scene.tmj");
tiled.log("asset=" + map.className + " " + map.width + "x" + map.height);
tiled.log("tile00=" + JSON.stringify(map.tileToPixelCoords(0, 0)));
tiled.log("tile10=" + JSON.stringify(map.tileToPixelCoords(1, 0)));
tiled.log("tile01=" + JSON.stringify(map.tileToPixelCoords(0, 1)));
const layer = map.layers.find(l => l.name === "objects");
const sample = layer.objects.slice(0, 5).map(o => ({ name: o.name, x: o.x, y: o.y, width: o.width, height: o.height }));
tiled.log("objects=" + JSON.stringify(sample));
tiled.log("firstWall=" + JSON.stringify(map.layers.find(l => l.name === "walls")));
