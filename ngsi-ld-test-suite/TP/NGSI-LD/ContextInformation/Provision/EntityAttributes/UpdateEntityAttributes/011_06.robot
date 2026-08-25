*** Settings ***
Documentation       Check that you can update the types on an entity

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Initial Entity
Test Teardown       Delete Initial Entity
Test Template       Append Types to an Entity


*** Variables ***
${entity_filename}=     building-minimal.json


*** Test Cases ***    FRAGMENT_FILENAME    EXPECTATION_FILENAME
011_06_01 AppendOneType
    type-vehicle-fragment.json    building-minimal-with-two-types.json
011_06_02 AppendTwoTypes
    types-vehicle-car-fragment.json    building-minimal-with-three-types.json
011_06_03 AppendExistingType
    type-building-fragment.json    building-minimal-compacted.json
011_06_04 AppendOneTypeAndOneExisting
    type-vehicle-building-fragment.json    building-minimal-with-two-types.json


*** Keywords ***
Append Types to an Entity
    [Documentation]    Check that type is appended if it is not yet in the target entity
    [Tags]    ea-append    5_6_2    4_16    since_v1.5.1
    [Arguments]    ${type_fragment_filename}    ${expectation_filename}

    ${response}=    Update Entity Attributes
    ...    ${entity_id}
    ...    ${type_fragment_filename}
    ...    ${CONTENT_TYPE_LD_JSON}

    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${response}=    Retrieve Entity
    ...    ${entity_id}
    ...    accept=${CONTENT_TYPE_JSON}
    ...    context=${ngsild_test_suite_context}
    ${entity_expectation_payload}=    Load Test Sample    entities/expectations/${expectation_filename}    ${entity_id}
    Check Updated Resource Set To    ${entity_expectation_payload}    ${response.json()}

Create Initial Entity
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${entity_filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_JSON}
    ...    ${ngsild_test_suite_context}
    Check Response Status Code    201    ${response.status_code}

Delete Initial Entity
    Delete Entity    ${entity_id}
