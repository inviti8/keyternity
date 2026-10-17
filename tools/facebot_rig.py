"""Convert FaceBot's rig (FaceBot.FBX) into the Face Rig panel's graphics
(docs/LIP_SYNC.md §6.5): each manipulator, and the mask behind them, flattened to
2D triangles in the mask's front plane, with their colours.

Usage: python tools/facebot_rig.py path/to/FaceBot.FBX assets/face_rig/facebot.json

The FBX is the author's own (REFERENCE/FaceBot_Free); only this derived data ships.
Coordinates are in the mask's units: x across (the character's left is +x), y up,
with the mask centred on x = 0. Each control's `rest` is its zero group's position,
and its triangles are relative to it.
"""
import json, struct, sys, zlib

# FaceBot's manipulators → the face rig's control names (src/face.rs LAYOUT)
CONTROLS = {
    'facebot_ctrl_L_brow': 'Brow_L', 'facebot_ctrl_R_brow': 'Brow_R',
    'facebot_ctrl_L_eye': 'Eye_L', 'facebot_ctrl_R_eye': 'Eye_R',
    'facebot_ctrl_L_nose': 'Nose_L', 'facebot_ctrl_R_nose': 'Nose_R',
    'facebot_ctrl_Mouth': 'Mouth',
    'facebot_ctrl_L_Mouth': 'Corner_L', 'facebot_ctrl_R_Mouth': 'Corner_R',
    'facebot_ctrl_U_Lip': 'UpperLip', 'facebot_ctrl_L_Lip': 'LowerLip',
    'facebot_ctrl_Jaw': 'Jaw', 'facebot_ctrl_Tongue': 'Tongue',
}
MASK = ['facebot_ctrlBase', 'facebot_vizFace']
# where the tongue sits on the panel (its geometry is modelled at the origin)
TONGUE_REST = (0., -14.4)


def read(path):
    data = open(path, 'rb').read()
    version = struct.unpack('<I', data[23:27])[0]
    wide = version >= 7500

    def node(pos):
        if wide:
            end, nprops, _ = struct.unpack('<QQQ', data[pos:pos + 24]); pos += 24
        else:
            end, nprops, _ = struct.unpack('<III', data[pos:pos + 12]); pos += 12
        nlen = data[pos]; pos += 1
        if end == 0:
            return None, pos
        name = data[pos:pos + nlen].decode('latin-1'); pos += nlen
        props = []
        for _ in range(nprops):
            t = chr(data[pos]); pos += 1
            if t in 'YCIFDL':
                fmt = {'Y': '<h', 'C': '<?', 'I': '<i', 'F': '<f', 'D': '<d', 'L': '<q'}[t]
                n = struct.calcsize(fmt)
                props.append(struct.unpack(fmt, data[pos:pos + n])[0]); pos += n
            elif t in 'fdlib':
                ln, enc, clen = struct.unpack('<III', data[pos:pos + 12]); pos += 12
                raw = data[pos:pos + clen]; pos += clen
                if enc:
                    raw = zlib.decompress(raw)
                fmt = {'f': 'f', 'd': 'd', 'l': 'q', 'i': 'i', 'b': '?'}[t]
                props.append(list(struct.unpack('<%d%s' % (ln, fmt), raw)))
            elif t in 'SR':
                ln = struct.unpack('<I', data[pos:pos + 4])[0]; pos += 4
                raw = data[pos:pos + ln]; pos += ln
                props.append(raw.decode('latin-1') if t == 'S' else raw)
            else:
                raise ValueError(t)
        children = []
        while pos < end:
            child, pos = node(pos)
            if child is None:
                break
            children.append(child)
        return {'name': name, 'props': props, 'children': children}, end

    nodes, pos = [], 27
    while pos < len(data) - 200:
        n, pos = node(pos)
        if n is None:
            break
        nodes.append(n)
    return {n['name']: n for n in nodes}


def main(src, out):
    top = read(src)
    objects = {}
    for o in top['Objects']['children']:
        name = o['props'][1].split('\x00\x01')[0] if len(o['props']) > 1 and isinstance(o['props'][1], str) else ''
        p70 = {}
        for c in o['children']:
            if c['name'] == 'Properties70':
                for p in c['children']:
                    p70[p['props'][0]] = p['props'][4:]
        objects[o['props'][0]] = (o['name'], name, o)
    geo_of, mat_of = {}, {}
    for c in top['Connections']['children']:
        kind, a, b = c['props'][0], c['props'][1], c['props'][2]
        ka, kb = objects.get(a, ('',))[0], objects.get(b, ('',))[0]
        if ka == 'Geometry' and kb == 'Model':
            geo_of[b] = a
        if ka == 'Material' and kb == 'Model':
            mat_of.setdefault(b, []).append(a)

    def colour(model_id, default):
        for m in mat_of.get(model_id, []):
            for c in objects[m][2]['children']:
                if c['name'] == 'Properties70':
                    for p in c['children']:
                        if p['props'][0] in ('DiffuseColor', 'Diffuse'):
                            return [round(v * 255) for v in p['props'][4:7]]
        return default

    def mesh(model_id):
        g = objects[geo_of[model_id]][2]
        verts = idx = None
        for c in g['children']:
            if c['name'] == 'Vertices':
                verts = c['props'][0]
            if c['name'] == 'PolygonVertexIndex':
                idx = c['props'][0]
        pts = [(verts[i], verts[i + 1], verts[i + 2]) for i in range(0, len(verts), 3)]
        tris, poly = [], []
        for i in idx:
            if i < 0:
                poly.append(-i - 1)
                for k in range(1, len(poly) - 1):  # fan
                    tris.append((poly[0], poly[k], poly[k + 1]))
                poly = []
            else:
                poly.append(i)
        depth = sum(p[2] for p in pts) / max(len(pts), 1)
        return pts, tris, depth

    by_name = {v[1]: k for k, v in objects.items() if v[0] == 'Model'}
    # the mask's centre: the base's x middle
    base_pts, _, _ = mesh(by_name['facebot_ctrlBase'])
    cx = (min(p[0] for p in base_pts) + max(p[0] for p in base_pts)) / 2

    # a key light from the upper left, toward the viewer (+z faces the viewer)
    lx, ly, lz = -0.35, 0.55, 0.76
    ln = (lx * lx + ly * ly + lz * lz) ** 0.5
    light = (lx / ln, ly / ln, lz / ln)

    def flat(pts, tris, dx, dy):
        """2D triangles, farthest first, each with a shade from its 3D normal."""
        r = lambda v: round(v, 2)
        out = []
        for t in tris:
            a, b, c = (pts[i] for i in t)
            tri = [[r(p[0] - dx), r(p[1] - dy)] for p in (a, b, c)]
            # drop slivers seen edge-on
            (ax, ay), (bx, by), (qx, qy) = tri
            if abs((bx - ax) * (qy - ay) - (by - ay) * (qx - ax)) <= 1e-3:
                continue
            u = (b[0] - a[0], b[1] - a[1], b[2] - a[2])
            v = (c[0] - a[0], c[1] - a[1], c[2] - a[2])
            n = (u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0])
            nl = (n[0] ** 2 + n[1] ** 2 + n[2] ** 2) ** 0.5 or 1.
            lambert = abs(n[0] * light[0] + n[1] * light[1] + n[2] * light[2]) / nl
            shade = round(0.32 + 0.68 * lambert, 3)
            depth = (a[2] + b[2] + c[2]) / 3
            out.append((depth, tri, shade))
        out.sort(key=lambda x: x[0])
        return [o[1] for o in out], [o[2] for o in out]

    mask = []
    for name, default in zip(MASK, ([20, 20, 22], [90, 90, 95])):
        pts, tris, depth = mesh(by_name[name])
        tris2, shades = flat(pts, tris, cx, 0.)
        mask.append({'name': name, 'color': colour(by_name[name], default), 'depth': depth, 'tris': tris2, 'shade': shades})
    mask.sort(key=lambda m: m['depth'])

    controls = []
    for fb, name in CONTROLS.items():
        pts, tris, depth = mesh(by_name[fb])
        if name == 'Tongue':
            bx = (min(p[0] for p in pts) + max(p[0] for p in pts)) / 2
            by_ = (min(p[1] for p in pts) + max(p[1] for p in pts)) / 2
            rest = TONGUE_REST
            # lay the tongue flat: its geometry stands in the x/z plane
            pts = [(p[0], p[2] * 0.5, p[1]) for p in pts]
            by_ = (min(p[1] for p in pts) + max(p[1] for p in pts)) / 2
            dx, dy = bx, by_
        else:
            xs = [p[0] - cx for p in pts]
            ys = [p[1] for p in pts]
            rest = ((min(xs) + max(xs)) / 2, (min(ys) + max(ys)) / 2)
            dx, dy = rest[0] + cx, rest[1]
        controls.append({
            'name': name,
            'color': colour(by_name[fb], [200, 200, 200]),
            'depth': depth,
            'rest': [round(rest[0], 2), round(rest[1], 2)],
        })
        controls[-1]['tris'], controls[-1]['shade'] = flat(pts, tris, dx, dy)
    # draw far first (the jaw behind the lips); the tongue, modelled at the origin,
    # sits on the jaw, so it's drawn last
    controls.sort(key=lambda c: (c['name'] == 'Tongue', c['depth']))
    for c in controls + mask:
        c.pop('depth')

    rig = {'name': 'FaceBot', 'source': 'FaceBot.FBX (Neuromancy Technology, 2016)', 'mask': mask, 'controls': controls}
    with open(out, 'w', encoding='utf-8') as f:
        json.dump(rig, f, separators=(',', ':'))
    print('wrote', out, sum(len(c['tris']) for c in controls + mask), 'triangles')


if __name__ == '__main__':
    main(sys.argv[1], sys.argv[2])
