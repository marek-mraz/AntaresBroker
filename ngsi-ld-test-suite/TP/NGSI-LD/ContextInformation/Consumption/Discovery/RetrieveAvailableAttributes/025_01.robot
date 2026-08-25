*** Settings ***
Documentation       Check that one can retrieve a list of NGSI-LD attributes

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Entity
Test Teardown       Delete Initial Entity
Test Template       Retrieve Available Attributes


*** Variables ***
${filename}=    building-simple-attributes.json


*** Test Cases ***    CONTEXT    EXPECTATION_FILE
025_01_01 WithoutJsonLdContext
    [Tags]    ed-attrs    5_7_8
    ${EMPTY}    types/expectations/attribute-list-025-01-01.json
025_01_02 WithJsonLdContext
    [Tags]    ed-attrs    5_7_8
    ${ngsild_test_suite_context}    types/expectations/attribute-list-025-01-02.json


*** Keywords ***
Retrieve Available Attributes
    [Documentation]    Check that one can retrieve a list of NGSI-LD attributes
    [Arguments]    ${context}    ${expectation_file}
    ${response}=    Retrieve Attributes
    ...    context=${context}
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing AttributeList element    ${expectation_file}    ${response.json()}

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
