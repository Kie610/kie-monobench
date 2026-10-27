"""cargo build -p kmb-cli の後、python3 crates/kmb-io-stl/tests/obj_cli.py で実行。"""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


class ObjCliTest(unittest.TestCase):
    def test_exported_sample_has_valid_indexed_triangles(self):
        repo = Path(__file__).resolve().parents[3]
        binary = repo / 'target' / 'debug' / ('kmb.exe' if os.name == 'nt' else 'kmb')
        with tempfile.TemporaryDirectory() as folder:
            env = dict(os.environ, XDG_DATA_HOME=str(Path(folder) / 'data'))
            for args in [['sample', 'part.kmb'], ['run', 'part.kmb', 'io.export_obj', '--arg', 'unit=cm', '--out', 'part.obj']]:
                result = subprocess.run([str(binary), *args], cwd=folder, env=env, capture_output=True, text=True, timeout=60)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            vertices, normals, faces, names = [], [], [], []
            for line in (Path(folder) / 'part.obj').read_text(encoding='utf-8').splitlines():
                kind, *values = line.split()
                if kind == 'v': vertices.append(tuple(map(float, values)))
                elif kind == 'vn': normals.append(tuple(map(float, values)))
                elif kind == 'o': names.append(' '.join(values))
                elif kind == 'f': faces.append([tuple(map(int, v.split('//'))) for v in values])
            self.assertTrue(vertices and normals and faces and names)
            self.assertTrue(all(len(face) == 3 for face in faces))
            for face in faces:
                for vertex, normal in face:
                    self.assertTrue(1 <= vertex <= len(vertices))
                    self.assertTrue(1 <= normal <= len(normals))
            volume = 0.0
            for face in faces:
                a, b, c = [vertices[v-1] for v, _ in face]
                volume += (a[0]*(b[1]*c[2]-b[2]*c[1]) + a[1]*(b[2]*c[0]-b[0]*c[2]) + a[2]*(b[0]*c[1]-b[1]*c[0])) / 6
            self.assertGreater(volume, 0)
            self.assertFalse(list(Path(folder).glob('*.mtl')))


if __name__ == '__main__':
    unittest.main()
