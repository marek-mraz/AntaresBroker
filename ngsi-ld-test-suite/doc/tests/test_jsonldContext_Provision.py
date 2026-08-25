#!/usr/bin/env python
from unittest import TestCase
from doc.analysis.generaterobotdata import GenerateRobotData
from json import load, dump
from deepdiff import DeepDiff
from os.path import dirname, exists
from os import listdir, remove, makedirs


class TestContextServerProvision(TestCase):
    # 14 failed, 26 passed
    @classmethod
    def setUpClass(cls):
        TestContextServerProvision.folder_test_suites = dirname(dirname(dirname(__file__)))
        folder_results = f'{TestContextServerProvision.folder_test_suites}/doc/results'

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

    def test_050_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/jsonldContext/Provision/AddContext/050_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/jsonldContext/Provision/050_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_050_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_050_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/jsonldContext/Provision/AddContext/050_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/jsonldContext/Provision/050_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_050_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_050_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/jsonldContext/Provision/AddContext/050_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/jsonldContext/Provision/050_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_050_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_050_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/jsonldContext/Provision/AddContext/050_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/jsonldContext/Provision/050_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_050_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_051_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/jsonldContext/Provision/DeleteContext/051_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/jsonldContext/Provision/051_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_051_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_051_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/jsonldContext/Provision/DeleteContext/051_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/jsonldContext/Provision/051_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_051_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_051_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/jsonldContext/Provision/DeleteContext/051_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/jsonldContext/Provision/051_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_051_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_051_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/jsonldContext/Provision/DeleteContext/051_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/jsonldContext/Provision/051_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_051_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_051_05(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/jsonldContext/Provision/DeleteContext/051_05.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/jsonldContext/Provision/051_05.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_051_05.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_051_06(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/jsonldContext/Provision/DeleteContext/051_06.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/jsonldContext/Provision/051_06.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_051_06.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_051_07(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/jsonldContext/Provision/DeleteContext/051_07.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/jsonldContext/Provision/051_07.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_051_07.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_051_08(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/jsonldContext/Provision/DeleteContext/051_08.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/jsonldContext/Provision/051_08.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_051_08.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_051_09(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/jsonldContext/Provision/DeleteContext/051_09.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/jsonldContext/Provision/051_09.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_051_09.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)
