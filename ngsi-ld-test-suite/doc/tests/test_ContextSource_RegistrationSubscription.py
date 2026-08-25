#!/usr/bin/env python
from unittest import TestCase
from doc.analysis.generaterobotdata import GenerateRobotData
from json import load, dump
from deepdiff import DeepDiff
from os.path import dirname, exists
from os import listdir, remove, makedirs


class TestCSRegistrationSubscription(TestCase):
    # 14 failed, 26 passed
    @classmethod
    def setUpClass(cls):
        TestCSRegistrationSubscription.folder_test_suites = dirname(dirname(dirname(__file__)))
        folder_results = f'{TestCSRegistrationSubscription.folder_test_suites}/doc/results'

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

    def test_038_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/CreateContextSourceRegistrationSubscription/038_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/038_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_038_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_038_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/CreateContextSourceRegistrationSubscription/038_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/038_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_038_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_038_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/CreateContextSourceRegistrationSubscription/038_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/038_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_038_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_038_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/CreateContextSourceRegistrationSubscription/038_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/038_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_038_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_038_05(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/CreateContextSourceRegistrationSubscription/038_05.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/038_05.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_038_05.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_038_06(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/CreateContextSourceRegistrationSubscription/038_06.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/038_06.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_038_06.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_038_07(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/CreateContextSourceRegistrationSubscription/038_07.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/038_07.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_038_07.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_038_08(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/CreateContextSourceRegistrationSubscription/038_08.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/038_08.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_038_08.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_038_09(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/CreateContextSourceRegistrationSubscription/038_09.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/038_09.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_038_09.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_042_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/DeleteContextSourceRegistrationSubscription/042_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/042_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_042_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_042_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/DeleteContextSourceRegistrationSubscription/042_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/042_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_042_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_042_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/DeleteContextSourceRegistrationSubscription/042_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/042_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_042_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_047_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/ContextSourceRegistrationSubscriptionNotificationBehaviour/047_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/047_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_047_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_047_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/ContextSourceRegistrationSubscriptionNotificationBehaviour/047_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/047_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_047_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_047_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/ContextSourceRegistrationSubscriptionNotificationBehaviour/047_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/047_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_047_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_047_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/ContextSourceRegistrationSubscriptionNotificationBehaviour/047_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/047_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_047_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_047_05(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/ContextSourceRegistrationSubscriptionNotificationBehaviour/047_05.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/047_05.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_047_05.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_047_06(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/ContextSourceRegistrationSubscriptionNotificationBehaviour/047_06.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/047_06.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_047_06.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_047_07(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/ContextSourceRegistrationSubscriptionNotificationBehaviour/047_07.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/047_07.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_047_07.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_047_08(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/ContextSourceRegistrationSubscriptionNotificationBehaviour/047_08.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/047_08.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_047_08.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_047_09(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/ContextSourceRegistrationSubscriptionNotificationBehaviour/047_09.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/047_09.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_047_09.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_047_10(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/ContextSourceRegistrationSubscriptionNotificationBehaviour/047_10.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/047_10.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_047_10.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_047_11(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/ContextSourceRegistrationSubscriptionNotificationBehaviour/047_11.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/047_11.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_047_11.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_047_12(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/ContextSourceRegistrationSubscriptionNotificationBehaviour/047_12.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/047_12.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_047_12.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_047_13(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/ContextSourceRegistrationSubscriptionNotificationBehaviour/047_13.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/047_13.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_047_13.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_047_14(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/ContextSourceRegistrationSubscriptionNotificationBehaviour/047_14.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/047_14.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_047_14.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_047_15(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/ContextSourceRegistrationSubscriptionNotificationBehaviour/047_15.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/047_15.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_047_15.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_047_16(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/ContextSourceRegistrationSubscriptionNotificationBehaviour/047_16.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/047_16.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_047_16.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_041_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/QueryContextSourceRegistrationSubscriptions/041_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/041_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_041_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_041_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/QueryContextSourceRegistrationSubscriptions/041_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/041_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_041_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_041_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/QueryContextSourceRegistrationSubscriptions/041_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/041_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_041_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_041_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/QueryContextSourceRegistrationSubscriptions/041_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/041_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_041_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_040_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/RetrieveContextSourceRegistrationSubscription/040_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/040_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_040_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_040_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/RetrieveContextSourceRegistrationSubscription/040_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/040_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_040_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_040_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/RetrieveContextSourceRegistrationSubscription/040_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/040_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_040_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_039_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/UpdateContextSourceRegistrationSubscription/039_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/039_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_039_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_039_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/UpdateContextSourceRegistrationSubscription/039_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/039_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_039_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_039_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/UpdateContextSourceRegistrationSubscription/039_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/039_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_039_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_039_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/UpdateContextSourceRegistrationSubscription/039_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/039_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_039_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_039_05(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextSource/RegistrationSubscription/UpdateContextSourceRegistrationSubscription/039_05.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextSource/RegistrationSubscription/039_05.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_039_05.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)
