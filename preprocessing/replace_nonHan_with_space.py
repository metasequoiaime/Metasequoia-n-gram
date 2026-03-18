import regex as re
import os


def keep_chinese(text):
    """保留中文字符，其它字符替换为空格"""
    return re.sub(r"\P{Han}", " ", text)


def process_file(input_file, output_file):
    """读取输入文件，处理内容，写入输出文件"""
    # 确保输出文件的父目录存在，如果不存在则创建
    output_dir = os.path.dirname(output_file)
    if not os.path.exists(output_dir):
        os.makedirs(output_dir)  # 创建目录（包括中间的所有缺失目录）

    with open(input_file, "r", encoding="utf-8") as infile, open(
        output_file, "w", encoding="utf-8"
    ) as outfile:
        for line in infile:
            # 处理每一行的文本
            cleaned_line = keep_chinese(line)

            # 使用正则表达式按空格切分文本
            words = re.split(r"\s+", cleaned_line.strip())  # \s+ 表示一个或多个空格

            # 将每个词或字符写入新的一行
            for word in words:
                if word:  # 如果是非空词，才写入
                    outfile.write(word + "\n")


input_file = os.path.join(
    os.path.dirname(os.path.dirname(__file__)), "data/wiki_zh/AA/wiki_00"
)  # 原始文件路径
output_file = os.path.join(
    os.path.dirname(os.path.dirname(__file__)), "data/output/wiki_zh/AA/wiki_00_cleaned"
)  # 处理后文件路径

process_file(input_file, output_file)
