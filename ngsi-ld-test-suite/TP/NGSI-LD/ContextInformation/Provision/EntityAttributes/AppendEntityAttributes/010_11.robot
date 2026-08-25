*** Settings ***
Documentation       Check that you can append a VocabProperty property to an entity

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Initial Entity
Test Teardown       Delete Initial Entity
Test Template       Append Attributes To Entity


*** Variables ***
${filename}=    vehicle-minimal.jsonld


*** Test Cases ***    STATUS_CODE    FRAGMENT_FILENAME    EXPECTATION_FILENAME
010_11_01 Append A VocabProperty Property
    [Tags]    ea-append    5_6_3    4_5_20    since_v1.7.1
    204    vehicle-new-vocab-property-fragment.jsonld    vehicle-vocab-property-appended.jsonld


*** Keywords ***
Append Attributes To Entity
    [Documentation]    Check that you can append a VocabProperty property to an entity
    [Arguments]    ${status_code}    ${fragment_filename}    ${expectation_filename}
    ${response}=    Append Entity Attributes    ${entity_id}    ${fragment_filename}    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    ${status_code}    ${response.status_code}
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
