# Metasequoia n-gram

Metasequoia-gram is a project tries to collect lexicon and build n-gram dataset for IME(Input Method Editor) in Chinese.

This project includes 4 parts:

- corpus collection
- data preprocessing
- segmentation
- n-gram info counting

## Corpus collection

## Data preprocessing

对于句子中的非中文字符，进行过滤，只要是非中文的字符，一律将其扩充成连通块，然后，将这个连通块当作是一个分隔符，将句子切分成小句子。最后，预处理好的数据就都是一个个由纯粹的中文组成的单独的连通块。

## Segmentation

对上面的预处理好的数据进行分词。

## N-gram info counting

分别计算一元数据和二元数据。

- 一元数据
  - 纯粹单字在所有字符中出现的频率。
  - 单字和多字词在以词为单位分割出来的所有的 tokens 中出现的频率。
- 二元数据：
  - 按单字切分的 2-gram 数据。
  - 按词切分的 tokens 的 2-gram 数据。

## How to build and run

For windows, you can use pwsh7 like this:

```shell
python -m venv .venv
.venv\Scripts\activate
pip install -r requirements.txt
```

## Reference

- open-gram: <https://github.com/sunpinyin/open-gram>
