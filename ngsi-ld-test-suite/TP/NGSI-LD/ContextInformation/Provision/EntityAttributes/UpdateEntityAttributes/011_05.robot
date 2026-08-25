*** Settings ***
Documentation       Check that one can update a scope in an entity

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Teardown       Delete Initial Entity


*** Variables ***
${scope_fragment_filename}=     one-scope-fragment.json


*** Test Cases ***
011_05_01 Update Scope To An Entity Already Having A Scope
    [Documentation]    Check that scope is replaced if entity already has a scope
    [Tags]    ea-append    5_6_2    4_18    since_v1.5.1
    [Setup]    Create Initial Entity    building-minimal-with-one-scope.json
    ${response}=    Update Entity Attributes
    ...    ${entity_id}
    ...    ${scope_fragment_filename}
    ...    ${CONTENT_TYPE_JSON}
    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${expectation_filename}=    Set Variable    building-minimal-with-one-scope.json
    ${entity_expectation_payload}=    Load Test Sample    entities/expectations/${expectation_filename}    ${entity_id}
    ${response}=    Retrieve Entity
    ...    ${entity_id}
    ...    accept=${CONTENT_TYPE_JSON}
    ...    context=${ngsild_test_suite_context}
    Check Updated Resource Set To    ${entity_expectation_payload}    ${response.json()}

011_05_02 Update Scope To An Entity Not Having A Scope
    [Documentation]    Check that scope is not added if entity does not already have a scope
    [Tags]    ea-append    5_6_2    4_18    since_v1.5.1
    [Setup]    Create Initial Entity    building-minimal.json
    ${response}=    Update Entity Attributes
    ...    ${entity_id}
    ...    ${scope_fragment_filename}
    ...    ${CONTENT_TYPE_JSON}
    Check Response Status Code    207    ${response.status_code}
    ${expectation_filename}=    Set Variable    building-minimal-compacted.json
    ${entity_expectation_payload}=    Load Test Sample    entities/expectations/${expectation_filename}    ${entity_id}
    ${response}=    Retrieve Entity
    ...    ${entity_id}
    ...    accept=${CONTENT_TYPE_JSON}
    ...    context=${ngsild_test_suite_context}
    Check Updated Resource Set To    ${entity_expectation_payload}    ${response.json()}


*** Keywords ***
Create Initial Entity
    [Arguments]    ${filename}
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_JSON}
    ...    ${ngsild_test_suite_context}
    Check Response Status Code    201    ${response.status_code}

Delete Initial Entity
    Delete Entity    ${entity_id}
