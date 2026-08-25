#!/usr/bin/env python
from unittest import TestCase
from doc.analysis.generaterobotdata import GenerateRobotData
from json import load, dump
from deepdiff import DeepDiff
from os.path import dirname, exists
from os import listdir, remove, makedirs


class TestCSRegistration(TestCase):
    # 2 failed, 33 passed
    @classmethod
    def setUpClass(cls):
        TestCSRegistration.folder_test_suites = dirname(dirname(dirname(__file__)))
        folder_results = f'{TestCSRegistration.folder_test_suites}/doc/results'

        # Check that the folder '/results' exists and if not, create it
        if not exists(folder_results):
            makedirs(folder_results)
        else:
            # Delete the /results folder
            [remove(f'{folder_results}/{x}') for x in listdir(folder_results) if x.startswith('out')]

    def setUp(self) -> None:
        self.folder_test_suites = dirname(dirname(dirname(__file__)))

    def common_function(self, robot_file, expected_value, difference_file):
        data = GenerateRobotData(robot_file=robot_file,
                                 execdir=self.folder_test_suites)
        data.parse_robot()
        obtained_response = data.get_info()

        with open(expected_value, 'r') as file:
            expected_response = load(file)

        result = DeepDiff(t1=obtained_response, t2=expected_response, ignore_order=True)

        if len(result) != 0:
            # There are some differences
            with open(difference_file, 'w') as fp:
                dump(obj=obtained_response, indent=2, fp=fp)

            assert False, f'They are some difference between the expected and obtained dictionaries: \n {result}'

    def test_037_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Discovery/QueryContextSourceRegistrations/037_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Discovery/037_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_037_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_037_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Discovery/QueryContextSourceRegistrations/037_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Discovery/037_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_037_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_037_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Discovery/QueryContextSourceRegistrations/037_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Discovery/037_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_037_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_037_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Discovery/QueryContextSourceRegistrations/037_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Discovery/037_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_037_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_037_05(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Discovery/QueryContextSourceRegistrations/037_05.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Discovery/037_05.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_037_05.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_037_06(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Discovery/QueryContextSourceRegistrations/037_06.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Discovery/037_06.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_037_06.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_037_07(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Discovery/QueryContextSourceRegistrations/037_07.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Discovery/037_07.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_037_07.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_037_08(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Discovery/QueryContextSourceRegistrations/037_08.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Discovery/037_08.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_037_08.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_037_09(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Discovery/QueryContextSourceRegistrations/037_09.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Discovery/037_09.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_037_09.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_037_10(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Discovery/QueryContextSourceRegistrations/037_10.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Discovery/037_10.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_037_10.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_037_11(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Discovery/QueryContextSourceRegistrations/037_11.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Discovery/037_11.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_037_11.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_036_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Discovery/RetrieveContextSourceRegistration/036_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Discovery/036_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_036_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_036_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Discovery/RetrieveContextSourceRegistration/036_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Discovery/036_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_036_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_036_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Discovery/RetrieveContextSourceRegistration/036_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Discovery/036_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_036_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_036_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Discovery/RetrieveContextSourceRegistration/036_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Discovery/036_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_036_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_036_05(self):
        # self.fail("(036_05) Test Suite with Test Template, not yet implemented")
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Discovery/RetrieveContextSourceRegistration/036_05.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Discovery/036_05.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_036_05.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)
