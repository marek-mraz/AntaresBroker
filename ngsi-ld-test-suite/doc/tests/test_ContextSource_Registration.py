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

    def test_033_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Registration/RegisterContextSource/033_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Registration/033_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_033_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_033_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Registration/RegisterContextSource/033_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Registration/033_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_033_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_033_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Registration/RegisterContextSource/033_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Registration/033_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_033_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_033_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Registration/RegisterContextSource/033_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Registration/033_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_033_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_033_05(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Registration/RegisterContextSource/033_05.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Registration/033_05.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_033_05.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_033_06(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Registration/RegisterContextSource/033_06.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Registration/033_06.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_033_06.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_033_07(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Registration/RegisterContextSource/033_07.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Registration/033_07.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_033_07.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_033_08(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Registration/RegisterContextSource/033_08.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Registration/033_08.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_033_08.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_033_09(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Registration/RegisterContextSource/033_09.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Registration/033_09.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_033_09.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_033_10(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Registration/RegisterContextSource/033_10.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Registration/033_10.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_033_10.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_035_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Registration/DeleteContextSourceRegistration/035_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Registration/035_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_035_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_035_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Registration/DeleteContextSourceRegistration/035_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Registration/035_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_035_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_035_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Registration/DeleteContextSourceRegistration/035_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Registration/035_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_035_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_034_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Registration/UpdateContextSourceRegistration/034_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Registration/034_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_034_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_034_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Registration/UpdateContextSourceRegistration/034_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Registration/034_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_034_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_034_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Registration/UpdateContextSourceRegistration/034_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Registration/034_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_034_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_034_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Registration/UpdateContextSourceRegistration/034_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Registration/034_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_034_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_034_05(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Registration/UpdateContextSourceRegistration/034_05.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Registration/034_05.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_034_05.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_034_06(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/Registration/UpdateContextSourceRegistration/034_06.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/Registration/034_06.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_034_06.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)
