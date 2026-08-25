from re import compile, match, findall, MULTILINE
from difflib import SequenceMatcher


def get_string_difference(string1: str, string2: str) -> str:
    differ = SequenceMatcher(None, string1, string2)
    differences = differ.get_opcodes()
    diff_string = ""

    for tag, i1, i2, j1, j2 in differences:
        if tag == 'delete' or tag == 'replace':
            diff_string += string1[i1:i2]
        elif tag == 'insert' or tag == 'replace':
            diff_string += string2[j1:j2]

    return diff_string


class Markdown:
    def __init__(self, filename: str, previous_content: str):
        # Read the content of the input file
        with open(filename, 'r') as file:
            self.content = file.read()
        file.close()

        # Initial previous content
        if previous_content != '':
            # If there was a previous content in the file, take the difference to do the process
            self.content = get_string_difference(string1=previous_content, string2=self.content)

        self.data = {
            "suite": str(),
            "cases": list(),
            "steps": list()
        }

        self.markdown_content = str()

    def get_names(self):
        pattern1 = compile('^(\S+.*)$', MULTILINE)

        aux = findall(pattern=pattern1, string=self.content)

        special_lines = ['Response:', 'Request:', 'Mismatch:', f'{"=" * 150}', f'{"=" * 80}', f'{"-" * 80}']
        xs = [x for x in aux if x not in special_lines]

        prefixes_to_remove = ["Item ", "+ ", "- ", "Value of ", "HTTP status code", "HTTPError:", "AttributeError:"]
        xs = [item for item in xs if not any(item.startswith(prefix) for prefix in prefixes_to_remove)]

        # Get the name of the Test Suite
        self.data["suite"] = xs[0]

        # Get the names of the Test Cases
        try:
            pattern = r"\d{3}\w+"
            self.data["cases"] = [item for item in xs if match(pattern, item)]
        except IndexError as err:
            print(f'\n[ERROR] Unexpected {err=}, {type(err)=} in TC {self.suite_name}\n\n')

        # Get the rest of values -> Steps
        # Get items from listA not present in listB and not equal to exclude_string
        self.data['steps'] = [item for item in xs if item not in self.data['cases'] and item != self.data['suite']]
        self.data['steps'] = list(set(self.data['steps']))

    def generate_md(self):
        # Replace the title of the Test Suite
        self.markdown_content = self.content
        self.markdown_content = (
            self.markdown_content.replace(f'{"=" * 150}\n{self.data["suite"]}\n{"=" * 150}', f'# {self.data["suite"]}'))

        # Replace the name of the Test Cases
        for x in self.data['cases']:
            self.markdown_content = (
                self.markdown_content.replace(f'{x}\n{"=" * 80}\n', f'```\n## {x}\n'))

        # Replace Request, Response, and Mismatch
        self.markdown_content = (self.markdown_content.replace(f'Request:\n{"-" * 80}', '#### Request:\n```')
                                 .replace(f'Response:\n{"-" * 80}', '```\n\n#### Response:\n```')
                                 .replace(f'Mismatch:\n{"-" * 80}', '```\n\n#### Mismatch:\n```'))

        # Replace the name of the steps
        for x in self.data['steps']:
            self.markdown_content = (
                self.markdown_content.replace(f'{x}\n', f'```\n### {x}\n'))

        # Final steps, correct the code style for the title of the Test Cases
        # Define patterns and replacement strings
        index = True
        for x in self.data['cases']:
            if index:
                self.markdown_content = (
                    self.markdown_content.replace(f'```\n## {x}\n\n```\n', f'## {x}\n\n'))
                index = False
            else:
                self.markdown_content = (
                    self.markdown_content.replace(f'```\n## {x}\n\n```\n', f'```\n## {x}\n\n'))

        # If the final number of "```" is odd, means that we need to close the last code section
        # this is a workaround to close the last section of code if this is keep open
        count = self.markdown_content.count("```")
        if count % 2 == 1:
            print(True)
            self.markdown_content = f"{self.markdown_content}\n```"

    def save_file(self, filename: str):
        # Write the Markdown content to the output file
        with open(filename, 'a') as file:
            file.write(self.markdown_content)
        file.close()

        return self.content

    def get_markdown(self) -> str:
        return self.markdown_content
