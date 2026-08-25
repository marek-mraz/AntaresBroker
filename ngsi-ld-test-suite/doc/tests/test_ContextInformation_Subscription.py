#!/usr/bin/env python
from unittest import TestCase
from doc.analysis.generaterobotdata import GenerateRobotData
from json import load, dump
from deepdiff import DeepDiff
from os.path import dirname, exists
from os import listdir, remove, makedirs


class TestCISubscription(TestCase):
    # 8 failed, 28 passed
    @classmethod
    def setUpClass(cls):
        TestCISubscription.folder_test_suites = dirname(dirname(dirname(__file__)))
        folder_results = f'{TestCISubscription.folder_test_suites}/doc/results'

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

    def test_028_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/CreateSubscription/028_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/028_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_028_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_028_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/CreateSubscription/028_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/028_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_028_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_028_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/CreateSubscription/028_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/028_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_028_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_028_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/CreateSubscription/028_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/028_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_028_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_028_05(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/CreateSubscription/028_05.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/028_05.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_028_05.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_028_06(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/CreateSubscription/028_06.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/028_06.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_028_06.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_028_07(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/CreateSubscription/028_07.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/028_07.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_028_07.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_028_08(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/CreateSubscription/028_08.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/028_08.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_028_08.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_032_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/DeleteSubscription/032_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/032_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_032_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_032_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/DeleteSubscription/032_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/032_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_032_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_032_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/DeleteSubscription/032_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/032_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_032_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_05(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_05.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_05.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_05.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_06(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_06.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_06.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_06.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_07(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_07.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_07.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_07.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_08(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_08.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_08.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_08.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_09(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_09.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_09.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_09.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_10(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_10.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_10.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_10.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_11(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_11.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_11.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_11.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_12(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_12.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_12.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_12.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_13(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_13.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_13.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_13.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_14(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_14.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_14.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_14.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_15(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_15.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_15.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_15.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_16_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_16_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_16_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_16_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_16_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_16_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_16_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_16_02.json'

    def test_046_17(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_17.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_17.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_17.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_18(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_18.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_18.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_18.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_19(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_19.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_19.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_19.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_20(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_20.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_20.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_20.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_21_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_21_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_21_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_21_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_21_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_21_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_21_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_21_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_22_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_22_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_22_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_22_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_22_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_22_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_22_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_22_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_22_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_22_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_22_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_22_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_22_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_22_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_22_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_22_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_22_05(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_22_05.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_22_05.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_22_05.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_22_06(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_22_06.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_22_06.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_22_06.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_22_07(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_22_07.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_22_07.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_22_07.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_22_08(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_22_08.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_22_08.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_22_08.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_22_09(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_22_09.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_22_09.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_22_09.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_22_10(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_22_10.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_22_10.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_22_10.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_22_11(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_22_11.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_22_11.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_22_11.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_22_12(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_22_12.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_22_12.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_22_12.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_22_13(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_22_13.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_22_13.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_22_13.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_23(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_23.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_23.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_23.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_24(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_24.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_24.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_24.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_25(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_25.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_25.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_25.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_26(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_26.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_26.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_26.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_27(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_27.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_27.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_27.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_28(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_28.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_28.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_28.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_29(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_29.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_29.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_29.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_30(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_30.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_30.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_30.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_31(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_31.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_31.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_31.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_32(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_32.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_32.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_32.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_33(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_33.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_33.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_33.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_34(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_34.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_34.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_34.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_35(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_35.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_35.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_35.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_36(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_36.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_36.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_36.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_37(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_37.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_37.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_37.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_38(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_38.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_38.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_38.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_39(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_39.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_39.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_39.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_40(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_40.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_40.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_40.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_41(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_41.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_41.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_41.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_42(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_42.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_42.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_42.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_046_43(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_43.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/046_43.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_046_43.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_031_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/QuerySubscriptions/031_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/031_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_031_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_031_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/QuerySubscriptions/031_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/031_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_031_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_030_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/RetrieveSubscription/030_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/030_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_030_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_030_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/RetrieveSubscription/030_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/030_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_030_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_030_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/RetrieveSubscription/030_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/030_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_030_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_029_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/UpdateSubscription/029_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/029_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_029_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_029_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/UpdateSubscription/029_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/029_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_029_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_029_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/UpdateSubscription/029_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/029_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_029_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_029_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/UpdateSubscription/029_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/029_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_029_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_029_05(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/UpdateSubscription/029_05.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/029_05.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_029_05.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_029_06(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/UpdateSubscription/029_06.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/029_06.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_029_06.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_029_07(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/UpdateSubscription/029_07.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/029_07.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_029_07.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_029_08(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/UpdateSubscription/029_08.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/029_08.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_029_08.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_029_09(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/UpdateSubscription/029_09.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/029_09.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_029_09.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_029_10(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/UpdateSubscription/029_10.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/029_10.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_029_10.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_029_11(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/UpdateSubscription/029_11.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/029_11.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_029_11.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_058_01(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/UpdateSubscription/058_01.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/058_01.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_058_01.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_058_02(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/UpdateSubscription/058_02.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/058_02.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_058_02.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_058_03(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/UpdateSubscription/058_03.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/058_03.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_058_03.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

    def test_058_04(self):
        robot_file = f'{self.folder_test_suites}/TP/NGSI-LD/ContextInformation/Subscription/UpdateSubscription/058_04.robot'
        expected_value = f'{self.folder_test_suites}/doc/files/ContextInformation/Subscription/058_04.json'
        difference_file = f'{self.folder_test_suites}/doc/results/out_058_04.json'

        self.common_function(robot_file=robot_file, expected_value=expected_value, difference_file=difference_file)

