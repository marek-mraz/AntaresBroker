#!/usr/bin/env python
from re import findall
from unittest import TestCase
from os.path import dirname, join, basename, splitext
from os import walk
from json import dumps


class TestCheckTests(TestCase):
    # 6 passed
    def setUp(self) -> None:
        folder_test_suites = dirname(dirname(dirname(__file__)))
        self.robot_files = list()
        self.test_lines = list()

        self.search_files_by_extension(
            directory=f'{folder_test_suites}/TP/NGSI-LD',
            extension='robot')

        self.search_lines_by_string_and_extension(
            directory=f'{folder_test_suites}/doc/tests',
            extension='py',
            string='def test')

        self.robots = [splitext(basename(x))[0] for x in self.robot_files]
        tests = [TestCheckTests.extract_number_test(string=x[1]) for x in self.test_lines]
        self.tests = [x for x in tests if x != '']

    def search_files_by_extension(self, directory, extension):
        for root, _, filenames in walk(directory):
            for filename in filenames:
                if filename.endswith(extension):
                    self.robot_files.append(join(root, filename))

    def search_lines_by_string_and_extension(self, directory, extension, string):
        for root, _, filenames in walk(directory):
            for filename in filenames:
                if filename.endswith(extension):
                    file_path = join(root, filename)
                    with open(file_path, 'r') as file:
                        for line in file:
                            if string in line:
                                self.test_lines.append((file_path, line.strip()))

    @staticmethod
    def extract_number_test(string: str) -> str:
        match = findall(pattern=r"[a-zA-Z ]+_([0-9_]+)", string=string)
        if len(match) == 1:
            return match[0]
        else:
            return ''

    def test_number_files_equal_number_tests(self):
        print("checking")
        number_robot_files = len(self.robot_files)
        number_test_lines = len(self.test_lines) - 4
        assert number_robot_files == number_test_lines, \
            (f"The number of robot files '{number_robot_files}' "
             f"is not the same as number of test cases '{number_test_lines}'")

    def test_specific_robot_file_has_a_test(self):
        check = [item for item in self.robots if item not in self.tests]
        check = [item for item in self.robot_files if any(string in item for string in check)]
        check = dumps(check, indent=4)

        assert check == '[]', f"The following robot files are missing from unittest:\n '{check}'"

    def test_that_all_tests_have_the_corresponding_robot_files(self):
        check = [item for item in self.tests if item not in self.robots]
        check = [item for item in self.test_lines if any(string in item[1] for string in check)]

        assert len(check) == 0, f"The following unit tests are missing their robot files:\n '{check}'"
