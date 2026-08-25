*** Settings ***
Documentation       Check that one cannot retrieve a detailed representation of an unknown NGSI-LD attribute

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Entity
Suite Teardown      Delete Initial Entity


*** Variables ***
${filename}=    building-simple-attributes.json


*** Test Cases ***
027_01_01 Retrieve Detailed Representation Of Available Attribute Without Context
    [Documentation]    Check that one cannot retrieve a detailed representation of an unknown NGSI-LD attribute
    [Tags]    ed-attr    5_7_10
    ${response}=    Retrieve Attribute    attribute_name=airQualityLevel
    Check Response Status Code    404    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_RESOURCE_NOT_FOUND}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}


*** Keywords ***
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
