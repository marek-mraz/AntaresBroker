*** Settings ***
Documentation       Check that one can retrieve a list with a detailed representation of NGSI-LD attributes

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Entity
Test Teardown       Delete Initial Entity
Test Template       Retrieve Details Of Available Attributes


*** Variables ***
${filename}=    building-simple-attributes.json


*** Test Cases ***    CONTEXT    EXPECTATION_FILE
026_01_01 WithoutJsonLdContext
    [Tags]    ed-attrs-details    5_7_9
    ${EMPTY}    types/expectations/attribute-026-01-01.json
026_01_02 WithJsonLdContext
    [Tags]    ed-attrs-details    5_7_9
    ${ngsild_test_suite_context}    types/expectations/attribute-026-01-02.json


*** Keywords ***
Retrieve Details Of Available Attributes
    [Documentation]    Check that one can retrieve a list with a detailed representation of NGSI-LD attributes
    [Arguments]    ${context}    ${expectation_file}
    ${response}=    Retrieve Attributes
    ...    context=${context}
    ...    details=true
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing Attribute element    ${expectation_file}    ${response.json()}

Setup Initial Entity
    ${entity_id}=    Generate Random Building Entity Id
    ${create_response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_JSON}
    ...    ${ngsild_test_suite_context}
    Check Response Status Code    201    ${create_response.status_code}
    Set Test Variable    ${entity_id}

Delete Initial Entity
    Delete Entity    ${entity_id}
