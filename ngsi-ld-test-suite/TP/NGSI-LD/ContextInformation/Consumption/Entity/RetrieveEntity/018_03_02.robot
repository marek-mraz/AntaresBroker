*** Settings ***
Documentation       Check that one cannot get an entity if an attribute is not known to the system

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Create Entity
Suite Teardown      Delete Created Entity


*** Variables ***
${filename}=                building-simple-attributes.jsonld
${attribute_not_known}=     property_not_found


*** Test Cases ***
018_03_02 Get An Entity If An Attribute Is Not Known To The System
    [Documentation]    Check that one cannot get an entity if an attribute is not known to the system
    [Tags]    e-retrieve    5_7_1
    ${attributes_to_be_retrieved}=    Catenate    SEPARATOR=,    ${attribute_not_known}

    ${response}=    Retrieve Entity
    ...    id=${entity_id}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    ...    attrs=${attributes_to_be_retrieved}

    Check Response Status Code    404    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_RESOURCE_NOT_FOUND}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}


*** Keywords ***
Create Entity
    ${entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${entity_id}
    ${create_response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response.status_code}

Delete Created Entity
    Delete Entity    ${entity_id}
