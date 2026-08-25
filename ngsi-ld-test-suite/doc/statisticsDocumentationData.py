from genericpath import exists
import os
from generateDocumentationData import create_json_of_robotfile
from os.path import dirname, join
from os import walk
from requests import delete, post
import json
import re
from analysis.initial_setup import InitialSetup

def locate_result_folder():
    folder_test_suites = dirname(dirname(__file__)).replace("\\", "/")
    folder_result_path = f"{folder_test_suites}/doc/results"
    if not exists(folder_result_path):
        return None
    return folder_result_path

if __name__ == "__main__":
    basedir = dirname(dirname(__file__))
    statistics = dict()
    testcases = []
    number_of_failures = 0
    number_of_all_testcases = 0
    number_of_successes = 0
    ROBOT_FILE_EXTENSION = ".robot"
    BASE_URL_OF_FORGE = (
        "https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/blob/master/TP/NGSI-LD/"
    )

    result_path = locate_result_folder()
    if result_path is not None:
        #delete the folder and all its content
        for root, dirs, files in walk(result_path, topdown=False):
            for name in files:
                os.remove(join(root, name))
            for name in dirs:
                os.rmdir(join(root, name))  

    fullpath = [
        (basedir + "/TP/NGSI-LD", "TP"),
        (basedir + "/IOP_TP/NGSI-LD", "IOP")
    ]

    excluded_dirs = [""]
    for path_info, label in fullpath:
        for root, dirs, files in walk(path_info):
            dirs[:] = [d for d in dirs if d not in excluded_dirs]
            for filename in files:
                if filename.endswith(ROBOT_FILE_EXTENSION):
                    number_of_all_testcases += 1
                    name_of_test_case = filename[: -len(ROBOT_FILE_EXTENSION)]
                    json_of_test_case = create_json_of_robotfile(name_of_test_case, True)
                    statistics[name_of_test_case] = dict()
                    strippedpath = root[len(path_info) + 1 :]
                    statistics[name_of_test_case]["path"] = strippedpath
                if (
                    "error_while_parsing" in json_of_test_case
                    and json_of_test_case["error_while_parsing"]
                ):
                    statistics[name_of_test_case]["failed"] = True
                    number_of_failures += 1
                    # we create a dummy entry in the "sub" test_cases, which has a "permutation_tp_id" equal to the
                    # robotfile. We do not forget to add a trailing slash that will be removed later, and a tail _XX
                    # which will allow matching from the googlesheet?
                    json_of_test_case["test_cases"] = [
                        {
                            "permutation_tp_id": "/"
                            + json_of_test_case["robotfile"]
                            + "_XX"
                        }
                    ]
                else:
                    statistics[name_of_test_case]["failed"] = False
                    number_of_successes += 1
                    # we add it here because Fernando's code does not, in case of successful parsing
                    json_of_test_case["error_while_parsing"] = False
                    if json_of_test_case["robotpath"].startswith("Interoperability"):
                            json_of_test_case["config_id"] = "CF_06"
                    elif json_of_test_case["robotpath"].startswith("DistributedOperations"):
                        json_of_test_case["config_id"] = "CF_04"
                    elif json_of_test_case["robotpath"].startswith("ContextSource"):
                        json_of_test_case["config_id"] = "CF_03"
                    elif json_of_test_case["robotpath"].startswith("ContextInformation/Subscription/SubscriptionNotificationBehaviour"):
                        json_of_test_case["config_id"] = "CF_02"
                    else:
                        json_of_test_case["config_id"] = "CF_01"

                    # upgrade the version and add the reference in square brackets
                    json_of_test_case["reference"] = re.sub(
                        r"V1\.[3-6]\.1 \[\]",
                        "V1.6.1 [1]",
                        json_of_test_case["reference"],
                    )

                    # now for each permutation inside this test case, create the permutation's correct parent_release
                    if "test_cases" in json_of_test_case:
                        # grab everything that is a permutation_body inside the "sub" test_cases,
                        for permutation_body in json_of_test_case["test_cases"]:
                            # default parent release
                            parent_release = "v1.3.1"
                            for tag in permutation_body["tags"]:
                                if tag.startswith("since_"):
                                    parts = tag.split("_")
                                    # the suffix
                                    parent_release = parts[-1]
                            permutation_body["permutation_parent_release"] = (
                                parent_release
                            )
                    else:
                        print(
                            "NO PERMUTATIONS in TESTCASE??? "
                            + json_of_test_case["tp_id"]
                        )
                        exit(1)
                testcases.append(json_of_test_case)
    print()
    print()
    print()
    print("THE FOLLOWING TESTCASES FAILED PARSING:")
    for testcasename, testcaseresult in statistics.items():
        if testcaseresult["failed"]:
            print(testcasename + " " + testcaseresult["path"])

    print(
        f"Out of {number_of_all_testcases} testcases, {number_of_failures} of them failed to be correctly parsed."
    )

    testcases_file = join(basedir, "doc", "results", "testcases.json")
    with open(testcases_file, "w") as fp:
        json.dump(obj=testcases, indent=2, fp=fp)

    # determine the structure/schema of a successfully parsed testcase
    permutation_template = {}
    for testcase in testcases:
        if not testcase["error_while_parsing"]:
            permutation_metadata_template = {}
            # everything that is at the top level shall be extracted
            for key, value in testcase.items():
                if key != "test_cases":
                    if type(key) is str:
                        permutation_metadata_template[key] = "UNKNOWN"
                    elif type(key) is list:
                        permutation_metadata_template[key] = []
                    elif type(key) is int:
                        permutation_metadata_template[key] = 0
                    elif type(key) is float:
                        permutation_metadata_template[key] = 0.0
                    elif type(key) is dict:
                        permutation_metadata_template[key] = {}
                    else:
                        print("UNKNOWN type")
                        exit(1)
            if "test_cases" in testcase:
                # everything that is a permutation_body inside the "sub" test_cases,
                # shall rise on its own existenz and be joined with its permutation_metadata
                for permutation_body in testcase["test_cases"]:
                    permutation_body_template = (
                        {}
                    )  # new object, not changing permutation_body
                    if "permutation_tp_id" in permutation_body:
                        permutation_body_template["stripped_permutation_tp_id"] = (
                            "UNKNOWN"
                        )
                        permutation_body_template["robotlink"] = "UNKNOWN"
                        for key, value in permutation_body.items():
                            if type(key) is str:
                                permutation_body_template[key] = "UNKNOWN"
                            elif type(key) is list:
                                permutation_body_template[key] = []
                            elif type(key) is int:
                                permutation_body_template[key] = 0
                            elif type(key) is float:
                                permutation_body_template[key] = 0.0
                            elif type(key) is dict:
                                permutation_body_template[key] = {}
                            else:
                                print("UNKNOWN BODY type")
                                exit(1)
                        # we use the unpacking python operator ** that strips the container dict from both
                        permutation_template = {
                            **permutation_metadata_template,
                            **permutation_body_template,
                        }
                    else:
                        print("NO PERMUTATION TP ID")
                        exit(1)
            else:
                print("TEMPLATE PARSING NOT FAILED, BUT no permutations??")
                exit(1)
            if permutation_template != {}:
                break

    print()
    print("Typical template:")
    print(permutation_template)

    # unpack all permutations of testcases that are under the same .robot file
    permutations = []
    for testcase in testcases:
        # print("--parsing "+testcase["robotfile"])
        permutation_metadata = {}

        # everything that is at the top level shall be extracted
        for key, value in testcase.items():
            if key != "test_cases":
                permutation_metadata[key] = value

        # start creating HTML link to robot file in repo
        fullurl = (
            BASE_URL_OF_FORGE
            + permutation_metadata["robotpath"]
            + "/"
            + permutation_metadata["robotfile"]
            + ROBOT_FILE_EXTENSION
        )

        if "test_cases" in testcase:
            # everything that is a permutation_body inside the "sub" test_cases,
            # shall rise on its own existenz and be joined with its permutation_metadata
            for permutation_body in testcase["test_cases"]:
                if "permutation_tp_id" in permutation_body:
                    ptpid = permutation_body["permutation_tp_id"]
                    if "then" not in permutation_body:
                        print(" no then in " + ptpid)
                    if "when" not in permutation_body:
                        print(" no when in " + ptpid)

                    # print("::: "+ptpid)
                    # strip from beginning up to including the last "/"
                    permutation_body["stripped_permutation_tp_id"] = ptpid[
                        ptpid.rindex("/") + 1 :
                    ]

                    # use the stripped_permutation_tp_id as text of the link
                    permutation_body["robotlink"] = (
                        '<a href="'
                        + fullurl
                        + '">'
                        + permutation_body["stripped_permutation_tp_id"]
                        + "</a>"
                    )

                    # So basically we append to the permutations a new dict that is the | union merge of the
                    # items of the template merged with the items of the concatenation of {**permutation_metadata,
                    # **permutation_body}. For this last concatenation we use the unpacking python operator ** that
                    # strips the container dict from both permutations.append(dict(permutation_template.items() |
                    # {**permutation_metadata, **permutation_body}.items()))
                    a = {**permutation_metadata, **permutation_body}
                    unpacked_testcase = {**permutation_template, **a}
                    # Perform a check on the clauses that must be equal to the
                    # clauses extracted from tags
                    if "clauses" not in unpacked_testcase:
                        print("NO EXTRACTED CLAUSES in " + ptpid)
                        exit(1)
                    if len(unpacked_testcase["clauses"]) > 1:
                        print("MULTIPLE CLAUSES in " + ptpid)
                    permutations.append(unpacked_testcase)
                elif "permutation_iop_id" in permutation_body:
                    # we do not want to add the IOP permutations to the list of permutations, 
                    # but we want to check that they are correctly parsed and that they have a robotlink
                    permutation_body["stripped_permutation_tp_id"] = ptpid[
                        ptpid.rindex("/") + 1 :
                    ]

                    # use the stripped_permutation_tp_id as text of the link
                    permutation_body["robotlink"] = (
                        '<a href="'
                        + fullurl
                        + '">'
                        + permutation_body["stripped_permutation_tp_id"]
                        + "</a>"
                    )
                else:
                    print("NO PERMUTATION TP ID")
                    exit(1)
        else:
            # there is no "sub" test_cases, it likely is a failed parsing
            if not testcase["error_while_parsing"]:
                print("PARSING NOT FAILED, BUT no permutations??")
                exit(1)

    permutations_file = join(basedir, "doc", "results", "permutations.json")
    with open(permutations_file, "w") as fp:
        json.dump(obj=permutations, indent=2, fp=fp)

    # Validate setup keys after all JSON files have been generated
    print("\nValidating setup keys...\n")
    initial_setup = InitialSetup()
    initial_setup.validate_setup_keys()
