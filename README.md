# Metasequoia n-gram

Metasequoia-gram is a project tries to collect lexicon and build n-gram dataset for IME(Input Method Editor) in Chinese.

This project includes 4 parts:

- corpus collection
- data preprocessing
- segmentation
- n-gram info counting

## Corpus collection

这里使用的语料来源是这个[仓库](https://github.com/brightmart/nlp_chinese_corpus)。将其解压后放到 data 目录下。

## Data preprocessing

对于句子中的非中文字符，进行过滤，只要是非中文的字符，一律将其扩充成连通块，然后，将这个连通块当作是一个分隔符，将句子切分成小句子。最后，预处理好的数据就都是一个个由纯粹的中文组成的单独的连通块。

```powershell
python .\preprocessing\generated_cleaned_txt\extract_connected_hanzi_components.py
python .\processing\generated_cleaned_txt\extract_wiki_connected_hanzi_components.py
python .\processing\generated_cleaned_txt\split_all_cleaned_txt_using_space.py
python .\processing\generated_cleaned_txt\split_wiki_txt_using_space.py
```

注意，这里是比较耗时的，至少需要半个小时左右，在我的 11 代 intel 处理器上。

## Segmentation

对上面的预处理好的数据进行分词。由于使用的是字级的 n-gram，所以，分词只需要将一个一个字分开即可。

## N-gram info counting

分别计算一元数据和二元数据。

- 一元数据：
  - 纯粹单字在所有字符中出现的频率。
- 二元数据：
  - 按单字切分的 2-gram 数据。
- 三元数据：
  - 按单字切分的 3-gram 数据。

这里的处理就都交给 kenlm 来做，kenlm 的编译和使用参见我的[笔记](https://luflyan.notion.site/VS2026-kenlm-32c722371a17802bac21f6b7e8eebf41?source=copy_link)。需要准备的东西是这种格式的文本语料：

```text
我 爱 北 京
我 爱 中 国
我 爱 上 海
北 京 是 中 国 首 都
```

需要将 [kenlm](https://github.com/kpu/kenlm) 项目编译好的 Release 版本的二进制文件复制到 kenlm_bin 目录下。然后，将生成的 model.arpa 和 model.binary 文件放到相应的 model\all 和 model\only_wiki 目录下。

```powershell
cmd /c ".\kenlm_bin\lmplz.exe -o 3 < .\data\output\all_cleaned_only_wiki_zh_spaced.txt > model.arpa"
.\kenlm_bin\build_binary.exe model.arpa model.binary
mkdir model\only_wiki
mv model.arpa model\only_wiki
mv model.binary model\only_wiki
```

```powershell
cmd /c ".\kenlm_bin\lmplz.exe -o 3 < .\data\output\all_cleaned_spaced.txt > model.arpa"
.\kenlm_bin\build_binary.exe model.arpa model.binary
mkdir model\all
mv model.arpa model\all
mv model.binary model\all
```

## How to build and run tests

For windows, you can use PowerShell 7.0+ like this:

```powershell
python -m venv .venv
.\.venv\Scripts\Activate.ps1
pip install regex
pip install https://github.com/kpu/kenlm/archive/master.zip
pip install opencc
```

注意，这里要用 Python 3.12。

跑测试代码之前，需要先生成一下拼音到单个汉字的候选项列表的字典文件，

```powershell
python .\preprocessing\generated_single_hanzi_pinyin_tbl\make_single_pinyin_table.py
```

然后，就可以运行测试了，

```powershell
python .\test\viterbi_no_pruning.py
```

## Reference

- open-gram: <https://github.com/sunpinyin/open-gram>
- nlp_chinese_corpus: <https://github.com/brightmart/nlp_chinese_corpus>
