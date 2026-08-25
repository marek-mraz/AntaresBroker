*** Settings ***
Documentation       Check that one can append entity attributes

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Initial Entity
Test Teardown       Delete Initial Entity
Test Template       Append Attributes Without Params


*** Variables ***
${filename}=    vehicle-speed-two-datasetid.jsonld


*** Test Cases ***    STATUS_CODE    FRAGMENT_FILENAME    EXPECTATION_FILENAME
010_01_01 Append Entity Attributes
    [Tags]    ea-append    5_6_3
    204    vehicle-new-attribute-fragment.jsonld    vehicle-speed-appended.jsonld
010_01_02 Append Entity Attributes With Different Datasetid
    [Tags]    ea-append    5_6_3
    204    vehicle-speed-different-datasetid-fragment.jsonld    vehicle-speed-different-datasetid.jsonld


*** Keywords ***
Append Attributes Without Params
    [Documentation]    Check that one can append entity attributes
    [Arguments]    ${status_code}    ${fragment_filename}    ${expectation_filename}
    ${response}=    Append Entity Attributes    ${entity_id}    ${fragment_filename}    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    ${status_code}    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${entity_expectation_payload}=    Load Test Sample    entities/expectations/${expectation_filename}    ${entity_id}
    ${response1}=    Retrieve Entity
    ...    id=${entity_id}
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    Check Updated Resource Set To
    ...    updated_resource=${entity_expectation_payload}
    ...    response_body=${response1.json()}

Create Initial Entity
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}
    Set Suite Variable    ${entity_id}

Delete Initial Entity
    Delete Entity    ${entity_id}
