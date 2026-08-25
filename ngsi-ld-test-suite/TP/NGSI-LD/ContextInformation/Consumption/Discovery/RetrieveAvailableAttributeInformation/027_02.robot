*** Settings ***
Documentation       Check that one can retrieve a list with a detailed representation of NGSI-LD attributes

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Entity
Suite Teardown      Delete Initial Entity
Test Template       Retrieve Detailed Representation Of Available Attribute


*** Variables ***
${filename}=            building-simple-attributes.json
${expectation_file}=    types/expectations/attribute-027-01.json


*** Test Cases ***    ATTR_NAME
027_02_01 WithCompactedAttributeName
    [Tags]    ed-attr    5_7_10
    airQualityLevel
027_02_02 WithExpandedAttributeName
    [Tags]    ed-attr    5_7_10
    https%3A%2F%2Fngsi-ld-test-suite%2Fcontext%23airQualityLevel


*** Keywords ***
Retrieve Detailed Representation Of Available Attribute
    [Documentation]    Check that one can retrieve a list with a detailed representation of NGSI-LD attributes
    [Arguments]    ${attributeName}
    ${response}=    Retrieve Attribute    attribute_name=${attributeName}    context=${ngsild_test_suite_context}
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
    Set Suite Variable    ${entity_id}

Delete Initial Entity
    Delete Entity    ${entity_id}
