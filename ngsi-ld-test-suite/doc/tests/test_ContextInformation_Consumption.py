#!/usr/bin/env python
from unittest import TestCase
from doc.analysis.generaterobotdata import GenerateRobotData
from json import load, dump
from deepdiff import DeepDiff
from os.path import dirname, exists
from os import listdir, remove, makedirs


class TestCIConsumptions(TestCase):
    # 58 passed
    @classmethod
    def setUpClass(cls):
        TestCIConsumptions.folder_test_suites = dirname(dirname(dirname(__file__)))
        folder_results = f'{TestCIConsumptions.folder_test_suites}/doc/results'

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

    def test_027_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Discovery/RetrieveAvailableAttributeInformation/027_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/027_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_027_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_027_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Discovery/RetrieveAvailableAttributeInformation/027_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/027_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_027_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_025_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Discovery/RetrieveAvailableAttributes/025_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/025_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_025_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_022_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Discovery/RetrieveAvailableEntityTypes/022_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/022_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_022_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_026_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Discovery/RetrieveDetailsOfAvailableAttributes/026_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/026_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_026_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_023_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Discovery/RetrieveDetailsOfAvailableEntityTypes/023_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/023_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_023_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_024_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Discovery/RetrieveAvailableEntityTypeInformation/024_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/024_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_024_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_024_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Discovery/RetrieveAvailableEntityTypeInformation/024_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/024_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_024_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_01_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_01_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_01_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_01_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_01_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_01_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_01_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_01_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_01_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_01_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_01_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_01_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_01_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_01_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_01_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_01_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_01_05(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_01_05.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_01_05.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_01_05.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_01_06(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_01_06.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_01_06.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_01_06.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_02_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_02_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_02_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_02_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_02_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_02_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_02_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_02_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_02_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_02_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_02_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_02_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_02_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_02_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_02_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_02_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_02_05(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_02_05.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_02_05.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_02_05.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_02_06(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_02_06.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_02_06.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_02_06.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_03_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_03_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_03_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_03_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_03_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_03_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_03_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_03_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_03_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_03_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_03_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_03_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_03_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_03_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_03_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_03_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_03_05(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_03_05.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_03_05.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_03_05.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_05(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_05.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_05.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_05.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_06(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_06.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_06.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_06.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_07(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_07.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_07.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_07.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_08(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_08.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_08.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_08.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_09(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_09.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_09.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_09.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_10(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_10.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_10.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_10.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_11(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_11.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_11.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_11.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_12(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_12.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_12.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_12.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_13(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_13.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_13.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_13.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_14(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_14.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_14.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_14.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_15(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_15.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_15.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_15.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_16(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_16.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_16.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_16.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_17(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_17.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_17.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_17.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_18(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_18.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_18.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_18.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_19(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_19.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_19.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_19.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_20(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_20.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_20.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_20.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_21(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_21.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_21.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_21.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_22(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_22.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_22.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_22.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_23(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_23.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_23.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_23.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_24(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_24.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_24.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_24.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_019_25(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/QueryEntities/019_25.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/019_25.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_019_25.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_018_01_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/RetrieveEntity/018_01_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/018_01_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_018_01_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_018_01_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/RetrieveEntity/018_01_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/018_01_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_018_01_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_018_01_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/RetrieveEntity/018_01_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/018_01_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_018_01_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_018_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/RetrieveEntity/018_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/018_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_018_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_018_03_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/RetrieveEntity/018_03_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/018_03_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_018_03_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_018_03_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/RetrieveEntity/018_03_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/018_03_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_018_03_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_018_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/RetrieveEntity/018_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/018_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_018_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_018_05(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/RetrieveEntity/018_05.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/018_05.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_018_05.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_018_06(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/RetrieveEntity/018_06.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/018_06.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_018_06.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_018_07(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/RetrieveEntity/018_07.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/018_07.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_018_07.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_018_08(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/RetrieveEntity/018_08.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/018_08.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_018_08.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_018_09(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/RetrieveEntity/018_09.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/018_09.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_018_09.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_018_10(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/RetrieveEntity/018_10.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/018_10.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_018_10.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_018_11(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/RetrieveEntity/018_11.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/018_11.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_018_11.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_018_12(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/RetrieveEntity/018_12.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/018_12.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_018_12.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_018_13(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/RetrieveEntity/018_13.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/018_13.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_018_13.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_018_14(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/RetrieveEntity/018_14.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/018_14.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_018_14.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_018_15(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/RetrieveEntity/018_15.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/018_15.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_018_15.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_018_16(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/RetrieveEntity/018_16.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/018_16.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_018_16.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_018_17(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/RetrieveEntity/018_17.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/018_17.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_018_17.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_018_18(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/RetrieveEntity/018_18.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/018_18.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_018_18.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_018_19(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/RetrieveEntity/018_19.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/018_19.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_018_19.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_018_20(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/RetrieveEntity/018_20.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/018_20.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_018_20.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_018_21(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/Entity/RetrieveEntity/018_21.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/018_21.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_018_21.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_021_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/QueryTemporalEvolutionOfEntities/021_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/021_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_021_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_021_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/QueryTemporalEvolutionOfEntities/021_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/021_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_021_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_021_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/QueryTemporalEvolutionOfEntities/021_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/021_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_021_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_021_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/QueryTemporalEvolutionOfEntities/021_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/021_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_021_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_021_05(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/QueryTemporalEvolutionOfEntities/021_05.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/021_05.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_021_05.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_021_06(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/QueryTemporalEvolutionOfEntities/021_06.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/021_06.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_021_06.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_021_07(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/QueryTemporalEvolutionOfEntities/021_07.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/021_07.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_021_07.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_021_08(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/QueryTemporalEvolutionOfEntities/021_08.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/021_08.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_021_08.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_021_09(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/QueryTemporalEvolutionOfEntities/021_09.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/021_09.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_021_09.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_021_11(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/QueryTemporalEvolutionOfEntities/021_11.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/021_11.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_021_11.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_021_12(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/QueryTemporalEvolutionOfEntities/021_12.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/021_12.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_021_12.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_021_13(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/QueryTemporalEvolutionOfEntities/021_13.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/021_13.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_021_13.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_021_14(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/QueryTemporalEvolutionOfEntities/021_14.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/021_14.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_021_14.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)
    def test_021_15(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/QueryTemporalEvolutionOfEntities/021_15.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/021_15.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_021_15.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_021_16(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/QueryTemporalEvolutionOfEntities/021_16.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/021_16.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_021_16.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_021_17(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/QueryTemporalEvolutionOfEntities/021_17.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/021_17.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_021_17.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_021_18(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/QueryTemporalEvolutionOfEntities/021_18.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/021_18.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_021_18.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_021_19(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/QueryTemporalEvolutionOfEntities/021_19.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/021_19.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_021_19.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_021_20(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/QueryTemporalEvolutionOfEntities/021_20.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/021_20.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_021_20.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_021_21(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/QueryTemporalEvolutionOfEntities/021_21.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/021_21.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_021_21.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_021_22(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/QueryTemporalEvolutionOfEntities/021_22.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/021_22.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_021_22.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_021_23(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/QueryTemporalEvolutionOfEntities/021_23.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/021_23.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_021_23.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_021_24(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/QueryTemporalEvolutionOfEntities/021_24.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/021_24.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_021_24.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_021_25(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/QueryTemporalEvolutionOfEntities/021_25.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/021_25.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_021_25.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_020_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/RetrieveTemporalEvolutionOfEntity/020_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/020_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_020_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_020_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/RetrieveTemporalEvolutionOfEntity/020_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/020_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_020_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_020_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/RetrieveTemporalEvolutionOfEntity/020_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/020_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_020_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_020_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/RetrieveTemporalEvolutionOfEntity/020_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/020_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_020_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_020_05(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/RetrieveTemporalEvolutionOfEntity/020_05.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/020_05.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_020_05.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_020_06(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/RetrieveTemporalEvolutionOfEntity/020_06.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/020_06.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_020_06.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_020_07(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/RetrieveTemporalEvolutionOfEntity/020_07.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/020_07.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_020_07.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_020_08(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/RetrieveTemporalEvolutionOfEntity/020_08.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/020_08.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_020_08.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_020_09(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/RetrieveTemporalEvolutionOfEntity/020_09.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/020_09.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_020_09.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_020_10(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/RetrieveTemporalEvolutionOfEntity/020_10.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/020_10.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_020_10.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_020_11(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/RetrieveTemporalEvolutionOfEntity/020_11.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/020_11.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_020_11.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_020_12(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/RetrieveTemporalEvolutionOfEntity/020_12.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/020_12.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_020_12.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_020_13(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/RetrieveTemporalEvolutionOfEntity/020_13.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/020_13.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_020_13.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_020_14(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/RetrieveTemporalEvolutionOfEntity/020_14.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/020_14.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_020_14.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_020_15(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/RetrieveTemporalEvolutionOfEntity/020_15.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/020_15.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_020_15.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_020_16(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/RetrieveTemporalEvolutionOfEntity/020_16.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/020_16.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_020_16.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_020_17(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/RetrieveTemporalEvolutionOfEntity/020_17.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/020_17.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_020_17.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_020_18(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/RetrieveTemporalEvolutionOfEntity/020_18.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/020_18.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_020_18.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_020_19(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/RetrieveTemporalEvolutionOfEntity/020_19.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/020_19.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_020_19.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_020_20(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/RetrieveTemporalEvolutionOfEntity/020_20.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/020_20.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_020_20.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_020_21(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/RetrieveTemporalEvolutionOfEntity/020_21.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/020_21.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_020_21.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_020_22(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/RetrieveTemporalEvolutionOfEntity/020_22.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/020_22.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_020_22.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_020_23(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/RetrieveTemporalEvolutionOfEntity/020_23.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Consumption/020_23.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_020_23.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)
