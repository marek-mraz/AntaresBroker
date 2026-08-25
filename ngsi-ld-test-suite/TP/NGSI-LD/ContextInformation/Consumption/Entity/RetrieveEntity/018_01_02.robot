*** Settings ***
Documentation       Check that one can query some attributes from an entity

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Create Entity
Suite Teardown      Delete Created Entity


*** Variables ***
${filename}=                        building-simple-attributes.jsonld
${expectation_filename}=            building-simple-attributes-query.jsonld
${attribute_airqualitylevel}=       https://ngsi-ld-test-suite/context#airQualityLevel
${attribute_subcategory}=           https://ngsi-ld-test-suite/context#subCategory


*** Test Cases ***
018_01_02 Query Some Attributes From An Entity
    [Documentation]    Check that one can query some attributes from an entity
    [Tags]    e-retrieve    5_7_1
    ${attributes_to_be_retrieved}=    Catenate
    ...    SEPARATOR=,
    ...    ${attribute_airqualitylevel}
    ...    ${attribute_subcategory}

    ${response}=    Retrieve Entity
    ...    id=${entity_id}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    ...    attrs=${attributes_to_be_retrieved}

    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing Entity element
    ...    ${expectation_filename}
    ...    ${entity_id}
    ...    ${response.json()}
    ...    ${True}


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
