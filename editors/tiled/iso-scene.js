/// <reference types="@mapeditor/tiled-api" />
//
// iso-scene collision check for Tiled.
//
// Install: copy this file into the Tiled extensions folder
// (Edit > Preferences > Plugins > Open extensions folder) and restart Tiled.
// Then Map > "Check iso-scene Collisions" selects every object that overlaps
// another object or stands on a wall, lists them in the Console, and redraws
// the hidden `collision` layer from the current object positions.
//
// It reads what `iso-scene tiled` writes: tile objects in the `objects` layer
// with `footprint_x/y/w/h` (in cells, relative to the object position) and
// `bottom`/`top`, and wall tile layers named `walls`, `walls-back` or
// `walls-front`.
//
// Without the editor: `tiled --evaluate iso-scene.js map.tmj` prints the
// collisions of each map.

var isoScene = (function () {
  var EPSILON = 1e-6;
  var WALL_LAYERS = ["walls", "walls-back", "walls-front"];

  function layersNamed(map, names) {
    var found = [];
    for (var i = 0; i < map.layerCount; ++i) {
      var layer = map.layerAt(i);
      if (names.indexOf(layer.name) >= 0) found.push(layer);
    }
    return found;
  }

  function number(object, name) {
    var value = object.property(name);
    return typeof value === "number" ? value : null;
  }

  // The footprint of an exported object in cells, or null for objects that
  // did not come from iso-scene.
  function footprint(object, tileHeight) {
    var fx = number(object, "footprint_x");
    var fy = number(object, "footprint_y");
    var fw = number(object, "footprint_w");
    var fh = number(object, "footprint_h");
    if (fx === null || fy === null || fw === null || fh === null) return null;
    var x0 = object.x / tileHeight + fx;
    var y0 = object.y / tileHeight + fy;
    return {
      object: object,
      area: [x0, y0, x0 + fw, y0 + fh],
      height: [number(object, "bottom") || 0, number(object, "top") || 0],
    };
  }

  function overlaps(a, b) {
    return (
      Math.max(a.area[0], b.area[0]) + EPSILON < Math.min(a.area[2], b.area[2]) &&
      Math.max(a.area[1], b.area[1]) + EPSILON < Math.min(a.area[3], b.area[3]) &&
      Math.max(a.height[0], b.height[0]) + EPSILON < Math.min(a.height[1], b.height[1])
    );
  }

  function cells(print) {
    return [
      Math.floor(print.area[0] + EPSILON),
      Math.floor(print.area[1] + EPSILON),
      Math.max(Math.ceil(print.area[2] - EPSILON), Math.floor(print.area[0] + EPSILON) + 1),
      Math.max(Math.ceil(print.area[3] - EPSILON), Math.floor(print.area[1] + EPSILON) + 1),
    ];
  }

  // Same rules as the exporter: objects collide when their footprints and
  // height ranges overlap; an object collides with every wall cell under it.
  // Objects are bucketed by cell, so only neighbours are compared.
  function findCollisions(map) {
    var tileHeight = map.tileHeight;
    var prints = [];
    layersNamed(map, ["objects"]).forEach(function (layer) {
      if (!layer.isObjectLayer) return;
      layer.objects.forEach(function (object) {
        var print = footprint(object, tileHeight);
        if (print) prints.push(print);
      });
    });
    var buckets = {};
    var collisions = [];
    var seen = {};
    var walls = layersNamed(map, WALL_LAYERS).filter(function (layer) {
      return layer.isTileLayer;
    });
    prints.forEach(function (print, index) {
      var c = cells(print);
      for (var y = c[1]; y < c[3]; ++y) {
        for (var x = c[0]; x < c[2]; ++x) {
          var key = x + "," + y;
          (buckets[key] = buckets[key] || []).forEach(function (other) {
            var pair = other + ":" + index;
            if (!seen[pair] && overlaps(prints[other], print)) {
              seen[pair] = true;
              collisions.push({ first: prints[other].object, second: print.object });
            }
          });
          buckets[key].push(index);
          walls.forEach(function (layer) {
            if (layer.tileAt(x, y)) {
              collisions.push({ first: print.object, wall: [x, y] });
            }
          });
        }
      }
    });
    return { footprints: prints, collisions: collisions };
  }

  function describe(collision) {
    var first = collision.first.name + " #" + collision.first.id;
    if (collision.wall) {
      return first + " stands on the wall at cell (" + collision.wall.join(", ") + ")";
    }
    return first + " overlaps " + collision.second.name + " #" + collision.second.id;
  }

  // Replace the rectangles of the `collision` layer with the current
  // footprints.
  function redrawFootprints(map, prints) {
    var layer = layersNamed(map, ["collision"])[0];
    if (!layer || !layer.isObjectLayer) return;
    while (layer.objectCount > 0) {
      layer.removeObjectAt(layer.objectCount - 1);
    }
    prints.forEach(function (print) {
      var shape = new MapObject(print.object.name);
      shape.className = "footprint";
      shape.x = print.area[0] * map.tileHeight;
      shape.y = print.area[1] * map.tileHeight;
      shape.width = (print.area[2] - print.area[0]) * map.tileHeight;
      shape.height = (print.area[3] - print.area[1]) * map.tileHeight;
      shape.setProperty("bottom", print.height[0]);
      shape.setProperty("top", print.height[1]);
      layer.addObject(shape);
    });
  }

  function check(map) {
    var result = findCollisions(map);
    map.macro("Check iso-scene collisions", function () {
      redrawFootprints(map, result.footprints);
    });
    var selected = [];
    result.collisions.forEach(function (collision) {
      tiled.log(describe(collision));
      [collision.first, collision.second].forEach(function (object) {
        if (object && selected.indexOf(object) < 0) selected.push(object);
      });
    });
    map.selectedObjects = selected;
    return result.collisions;
  }

  return { findCollisions: findCollisions, describe: describe, check: check };
})();

if (typeof tiled.registerAction === "function" && tiled.actions.indexOf("IsoSceneCheckCollisions") < 0) {
  var checkAction = tiled.registerAction("IsoSceneCheckCollisions", function () {
    var map = tiled.activeAsset;
    if (!map || !map.isTileMap) {
      tiled.alert("Open an iso-scene map first.");
      return;
    }
    var collisions = isoScene.check(map);
    if (collisions.length === 0) {
      tiled.log("iso-scene: no collisions");
    } else {
      tiled.alert(collisions.length + " collision(s); see the Console. The objects are selected.");
    }
  });
  checkAction.text = "Check iso-scene Collisions";
  try {
    tiled.extendMenu("Map", [{ separator: true }, { action: "IsoSceneCheckCollisions" }]);
  } catch (error) {
    // `tiled --evaluate` has no menus; the functions still work there.
  }
}

// `tiled --evaluate iso-scene.js map.tmj` checks a map without the editor.
if (tiled.scriptArguments && tiled.scriptArguments.length > 0) {
  tiled.scriptArguments.forEach(function (path) {
    var map = tiled.mapFormat("json").read(path);
    var collisions = isoScene.findCollisions(map).collisions;
    collisions.forEach(function (collision) {
      tiled.log(path + ": " + isoScene.describe(collision));
    });
    tiled.log(path + ": " + collisions.length + " collision(s)");
  });
}
