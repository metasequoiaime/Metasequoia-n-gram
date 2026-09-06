"""Exercise actual preprocessing functions without downloading a private/large corpus."""
import importlib.util
import io
from pathlib import Path
import tempfile
import unittest

SOURCE = Path(__file__).resolve().parents[1] / "preprocessing/generate_cleaned_txt"


def load(name):
    spec = importlib.util.spec_from_file_location(name, SOURCE / (name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class PreprocessingTests(unittest.TestCase):
    def test_clean_and_segment(self):
        for name in ("extract_connected_hanzi_components", "extract_wiki_connected_hanzi_components"):
            with self.subTest(module=name), tempfile.TemporaryDirectory() as directory:
                source = Path(directory) / "input.txt"
                source.write_text("水杉IME，输入法123！\nABC\n𠀀中文\n", encoding="utf-8")
                output = io.StringIO()
                load(name).process_file(source, output)
                self.assertEqual(output.getvalue(), "水杉\n输入法\n𠀀中文\n")

    def test_character_spacing(self):
        for name in ("split_all_cleaned_txt_using_space", "split_wiki_txt_using_space"):
            module = load(name)
            self.assertEqual(module.split_hanzi_with_space(" 水杉输入法\n"), "水 杉 输 入 法")
            self.assertEqual(module.split_hanzi_with_space("𠀀中文"), "𠀀 中 文")
            self.assertEqual(module.split_hanzi_with_space("\n"), "")

    def test_corpus_selection(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            (base / "wiki_zh/sub").mkdir(parents=True)
            wiki = base / "wiki_zh/sub/article.json"
            train = base / "news_train.json"
            for path in (wiki, train, base / "unrelated.json"):
                path.touch()
            all_inputs = {Path(p) for p in load("extract_connected_hanzi_components").iter_input_files(base)}
            wiki_inputs = {Path(p) for p in load("extract_wiki_connected_hanzi_components").iter_input_files(base)}
            self.assertEqual(all_inputs, {wiki, train})
            self.assertEqual(wiki_inputs, {wiki})


if __name__ == "__main__":
    unittest.main()
