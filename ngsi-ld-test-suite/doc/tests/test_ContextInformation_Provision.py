#!/usr/bin/env python
from unittest import TestCase
from doc.analysis.generaterobotdata import GenerateRobotData
from json import load, dump
from deepdiff import DeepDiff
from os.path import dirname, exists
from os import listdir, remove, makedirs


class TestCIProvision(TestCase):
    # 16 failed, 53 passed
    @classmethod
    def setUpClass(cls):
        TestCIProvision.folder_test_suites = dirname(dirname(dirname(__file__)))
        folder_results = f'{TestCIProvision.folder_test_suites}/doc/results'

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

    def test_003_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/CreateBatchOfEntities/003_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/003_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_003_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_003_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/CreateBatchOfEntities/003_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/003_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_003_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_003_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/CreateBatchOfEntities/003_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/003_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_003_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_003_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/CreateBatchOfEntities/003_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/003_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_003_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_003_05(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/CreateBatchOfEntities/003_05.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/003_05.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_003_05.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_003_06(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/CreateBatchOfEntities/003_06.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/003_06.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_003_06.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_003_07(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/CreateBatchOfEntities/003_07.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/003_07.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_003_07.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_003_08(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/CreateBatchOfEntities/003_08.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/003_08.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_003_08.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_003_09(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/CreateBatchOfEntities/003_09.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/003_09.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_003_09.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_003_10(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/CreateBatchOfEntities/003_10.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/003_10.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_003_10.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_006_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/DeleteBatchOfEntities/006_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/006_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_006_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_006_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/DeleteBatchOfEntities/006_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/006_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_006_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_006_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/DeleteBatchOfEntities/006_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/006_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_006_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_006_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/DeleteBatchOfEntities/006_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/006_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_006_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_005_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/UpdateBatchOfEntities/005_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/005_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_005_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_005_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/UpdateBatchOfEntities/005_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/005_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_005_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_005_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/UpdateBatchOfEntities/005_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/005_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_005_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_005_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/UpdateBatchOfEntities/005_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/005_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_005_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_005_05(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/UpdateBatchOfEntities/005_05.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/005_05.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_005_05.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_004_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/UpsertBatchOfEntities/004_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/004_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_004_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_004_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/UpsertBatchOfEntities/004_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/004_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_004_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_004_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/UpsertBatchOfEntities/004_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/004_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_004_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_004_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/UpsertBatchOfEntities/004_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/004_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_004_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_004_05(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/UpsertBatchOfEntities/004_05.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/004_05.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_004_05.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_004_06(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/UpsertBatchOfEntities/004_06.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/004_06.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_004_06.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_004_07(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/UpsertBatchOfEntities/004_07.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/004_07.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_004_07.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_004_08(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/UpsertBatchOfEntities/004_08.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/004_08.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_004_08.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_001_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/Entities/CreateEntity/001_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/001_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_001_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_001_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/Entities/CreateEntity/001_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/001_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_001_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_001_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/Entities/CreateEntity/001_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/001_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_001_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_001_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/Entities/CreateEntity/001_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/001_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_001_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_001_05(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/Entities/CreateEntity/001_05.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/001_05.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_001_05.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_001_06(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/Entities/CreateEntity/001_06.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/001_06.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_001_06.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_001_07(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/Entities/CreateEntity/001_07.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/001_07.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_001_07.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_001_08(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/Entities/CreateEntity/001_08.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/001_08.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_001_08.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_001_09(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/Entities/CreateEntity/001_09.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/001_09.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_001_09.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_001_10(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/Entities/CreateEntity/001_10.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/001_10.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_001_10.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_001_11(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/Entities/CreateEntity/001_11.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/001_11.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_001_11.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_001_12(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/Entities/CreateEntity/001_12.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/001_12.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_001_12.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_001_13(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/Entities/CreateEntity/001_13.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/001_13.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_001_13.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_001_14(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/Entities/CreateEntity/001_14.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/001_14.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_001_14.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_001_15(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/Entities/CreateEntity/001_15.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/001_15.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_001_15.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_002_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/Entities/DeleteEntity/002_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/002_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_002_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_002_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/Entities/DeleteEntity/002_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/002_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_002_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_002_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/Entities/DeleteEntity/002_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/002_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_002_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_010_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/AppendEntityAttributes/010_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/010_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_010_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_010_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/AppendEntityAttributes/010_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/010_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_010_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_010_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/AppendEntityAttributes/010_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/010_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_010_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_010_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/AppendEntityAttributes/010_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/010_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_010_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_010_05(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/AppendEntityAttributes/010_05.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/010_05.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_010_05.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_010_06(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/AppendEntityAttributes/010_06.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/010_06.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_010_06.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_010_07(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/AppendEntityAttributes/010_07.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/010_07.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_010_07.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_010_08(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/AppendEntityAttributes/010_08.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/010_08.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_010_08.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_010_09(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/AppendEntityAttributes/010_09.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/010_09.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_010_09.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_010_10(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/AppendEntityAttributes/010_10.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/010_10.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_010_10.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_010_11(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/AppendEntityAttributes/010_11.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/010_11.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_010_11.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_013_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/DeleteEntityAttribute/013_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/013_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_013_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_013_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/DeleteEntityAttribute/013_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/013_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_013_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_013_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/DeleteEntityAttribute/013_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/013_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_013_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_013_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/DeleteEntityAttribute/013_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/013_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_013_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_013_05(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/DeleteEntityAttribute/013_05.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/013_05.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_013_05.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_013_06(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/DeleteEntityAttribute/013_06.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/013_06.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_013_06.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_013_07(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/DeleteEntityAttribute/013_07.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/013_07.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_013_07.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_012_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/PartialAttributeUpdate/012_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/012_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_012_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_012_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/PartialAttributeUpdate/012_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/012_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_012_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_012_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/PartialAttributeUpdate/012_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/012_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_012_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_012_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/PartialAttributeUpdate/012_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/012_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_012_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_012_05(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/PartialAttributeUpdate/012_05.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/012_05.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_012_05.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_012_06(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/PartialAttributeUpdate/012_06.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/012_06.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_012_06.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_012_07(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/PartialAttributeUpdate/012_07.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/012_07.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_012_07.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_012_08(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/PartialAttributeUpdate/012_08.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/012_08.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_012_08.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_012_09(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/PartialAttributeUpdate/012_09.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/012_09.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_012_09.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_011_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/UpdateEntityAttributes/011_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/011_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_011_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_011_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/UpdateEntityAttributes/011_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/011_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_011_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_011_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/UpdateEntityAttributes/011_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/011_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_011_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_011_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/UpdateEntityAttributes/011_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/011_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_011_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_011_05(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/UpdateEntityAttributes/011_05.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/011_05.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_011_05.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_011_06(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/UpdateEntityAttributes/011_06.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/011_06.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_011_06.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_011_07(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/UpdateEntityAttributes/011_07.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/011_07.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_011_07.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_011_08(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/UpdateEntityAttributes/011_08.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/011_08.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_011_08.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_011_09(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/UpdateEntityAttributes/011_09.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/011_09.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_011_09.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_007_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/TemporalEntity/CreateTemporalRepresentationOfEntity/007_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/007_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_007_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_007_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/TemporalEntity/CreateTemporalRepresentationOfEntity/007_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/007_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_007_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_007_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/TemporalEntity/CreateTemporalRepresentationOfEntity/007_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/007_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_007_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_009_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/TemporalEntity/DeleteTemporalRepresentationOfEntity/009_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/009_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_009_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_009_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/TemporalEntity/DeleteTemporalRepresentationOfEntity/009_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/009_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_009_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_009_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/TemporalEntity/DeleteTemporalRepresentationOfEntity/009_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/009_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_009_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_008_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/TemporalEntity/UpdateTemporalRepresentationOfEntity/008_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/008_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_008_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_014_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/TemporalEntityAttributes/AddAttributes/014_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/014_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_014_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_014_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/TemporalEntityAttributes/AddAttributes/014_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/014_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_014_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_014_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/TemporalEntityAttributes/AddAttributes/014_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/014_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_014_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_014_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/TemporalEntityAttributes/AddAttributes/014_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/014_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_014_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_015_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/TemporalEntityAttributes/DeleteAttribute/015_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/015_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_015_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_015_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/TemporalEntityAttributes/DeleteAttribute/015_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/015_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_015_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_015_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/TemporalEntityAttributes/DeleteAttribute/015_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/015_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_015_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_017_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/TemporalEntityAttributes/DeleteAttributeInstance/017_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/017_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_017_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_017_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/TemporalEntityAttributes/DeleteAttributeInstance/017_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/017_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_017_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_017_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/TemporalEntityAttributes/DeleteAttributeInstance/017_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/017_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_017_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_016_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/TemporalEntityAttributes/PartialUpdateAttributeInstance/016_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/016_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_016_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_016_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/TemporalEntityAttributes/PartialUpdateAttributeInstance/016_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/016_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_016_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_016_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/TemporalEntityAttributes/PartialUpdateAttributeInstance/016_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/016_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_016_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_054_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/Entities/ReplaceEntity/054_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/054_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_054_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_054_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/Entities/ReplaceEntity/054_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/054_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_054_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_055_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/ReplaceEntityAttribute/055_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/055_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_055_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_055_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/ReplaceEntityAttribute/055_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/055_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_055_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_055_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/EntityAttributes/ReplaceEntityAttribute/055_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/055_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_055_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_057_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/UpdateBatchOfEntities/005_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/057_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_057_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_057_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/UpdateBatchOfEntities/057_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/057_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_057_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_057_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/UpdateBatchOfEntities/057_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/057_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_057_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_060_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/UpdateBatchOfEntities/060_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/060_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_060_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_060_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/UpdateBatchOfEntities/060_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/060_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_060_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_060_03_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/UpdateBatchOfEntities/060_03_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/060_03_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_060_03_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_060_03_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/UpdateBatchOfEntities/060_03_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/060_03_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_060_03_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_060_03_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/UpdateBatchOfEntities/060_03_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/060_03_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_060_03_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_060_03_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/UpdateBatchOfEntities/060_03_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/060_03_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_060_03_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_060_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/UpdateBatchOfEntities/060_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/060_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_060_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_060_05(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Provision/BatchEntities/UpdateBatchOfEntities/060_05.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Provision/060_05.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_060_05.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)
