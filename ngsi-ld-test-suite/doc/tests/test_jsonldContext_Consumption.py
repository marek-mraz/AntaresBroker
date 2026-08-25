#!/usr/bin/env python
from unittest import TestCase
from doc.analysis.generaterobotdata import GenerateRobotData
from json import load, dump
from deepdiff import DeepDiff
from os.path import dirname, exists
from os import listdir, remove, makedirs


class TestContextServerConsumption(TestCase):
    # 14 failed, 26 passed
    @classmethod
    def setUpClass(cls):
        TestContextServerConsumption.folder_test_suites = dirname(dirname(dirname(__file__)))
        folder_results = f'{TestContextServerConsumption.folder_test_suites}/doc/results'

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

    def test_052_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/jsonldContext/Consumption/ListContexts/052_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/jsonldContext/Consumption/052_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_052_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_052_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/jsonldContext/Consumption/ListContexts/052_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/jsonldContext/Consumption/052_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_052_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_052_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/jsonldContext/Consumption/ListContexts/052_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/jsonldContext/Consumption/052_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_052_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_052_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/jsonldContext/Consumption/ListContexts/052_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/jsonldContext/Consumption/052_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_052_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_052_05(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/jsonldContext/Consumption/ListContexts/052_05.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/jsonldContext/Consumption/052_05.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_052_05.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_052_06(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/jsonldContext/Consumption/ListContexts/052_06.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/jsonldContext/Consumption/052_06.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_052_06.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_052_07(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/jsonldContext/Consumption/ListContexts/052_07.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/jsonldContext/Consumption/052_07.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_052_07.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_053_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/jsonldContext/Consumption/ServeContext/053_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/jsonldContext/Consumption/053_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_053_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_053_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/jsonldContext/Consumption/ServeContext/053_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/jsonldContext/Consumption/053_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_053_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_053_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/jsonldContext/Consumption/ServeContext/053_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/jsonldContext/Consumption/053_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_053_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_053_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/jsonldContext/Consumption/ServeContext/053_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/jsonldContext/Consumption/053_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_053_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_053_05(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/jsonldContext/Consumption/ServeContext/053_05.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/jsonldContext/Consumption/053_05.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_053_05.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_053_06(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/jsonldContext/Consumption/ServeContext/053_06.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/jsonldContext/Consumption/053_06.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_053_06.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_053_07(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/jsonldContext/Consumption/ServeContext/053_07.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/jsonldContext/Consumption/053_07.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_053_07.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_053_08(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/jsonldContext/Consumption/ServeContext/053_08.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/jsonldContext/Consumption/053_08.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_053_08.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)
