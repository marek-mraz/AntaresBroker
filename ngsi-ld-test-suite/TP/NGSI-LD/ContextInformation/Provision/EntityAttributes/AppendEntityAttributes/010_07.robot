*** Settings ***
Documentation       Check that one can append a scope to an entity

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Initial Entity
Test Teardown       Delete Initial Entity


*** Variables ***
${filename}=                    building-minimal-with-one-scope.json
${scope_fragment_filename}=     one-scope-fragment.json


*** Test Cases ***
010_07_01 Append Scope To An Entity With Overwrite Enabled
    [Documentation]    Check that scope is replaced if overwrite is enabled
    [Tags]    ea-append    5_6_3    4_18    since_v1.5.1
    ${response}=    Append Entity Attributes
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

010_07_01 Append Scope To An Entity With Overwrite Disabled
    [Documentation]    Check that scope is appended if overwrite is disabled
    [Tags]    ea-append    5_6_3    4_18    since_v1.5.1
    ${response}=    Append Entity Attributes With Parameters
    ...    ${entity_id}
    ...    ${scope_fragment_filename}
    ...    ${CONTENT_TYPE_JSON}
    ...    noOverwrite
    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${expectation_filename}=    Set Variable    building-minimal-with-two-scopes.json
    ${entity_expectation_payload}=    Load Test Sample    entities/expectations/${expectation_filename}    ${entity_id}
    ${response}=    Retrieve Entity
    ...    ${entity_id}
    ...    accept=${CONTENT_TYPE_JSON}
    ...    context=${ngsild_test_suite_context}
    Check Updated Resource Set To    ${entity_expectation_payload}    ${response.json()}


*** Keywords ***
Create Initial Entity
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
