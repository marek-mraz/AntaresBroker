*** Settings ***
Documentation       Verify that a repeated query parameter is refused rather than
...                 silently resolved.
...
...                 ANTARES DECISION, NOT A CIM 009 REQUIREMENT. No clause says
...                 what a broker does when the same query parameter appears
...                 twice: 6.3.20 covers UNKNOWN parameters and 6.3.14 covers a
...                 repeated NGSILD-Tenant header, but neither covers a repeated
...                 parameter. Antares refuses it with InvalidRequest 400.
...
...                 The reason is the gateway seam. CIM 009 gives the broker no
...                 authorization model, so a policy layer sits in front of it,
...                 and implementations disagree on which occurrence of a
...                 repeated parameter wins (first, last, or the values joined).
...                 Resolving the ambiguity silently lets that layer authorize
...                 one value while the broker acts on another.
...
...                 Nothing a conformant client sends is affected: NGSI-LD passes
...                 a list as ONE parameter, and several Entity Types are a 4.17
...                 selector inside one `type` value.

Library             RequestsLibrary
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource


*** Test Cases ***
A Repeated Query Parameter Is Refused
    [Documentation]    The same parameter twice is an ambiguity the broker does
    ...    not resolve on the client's behalf, whatever the second value is.
    [Tags]    antares-specific    query-params    parameter-pollution
    Repeated Parameter Should Be Refused    type=Vehicle&type=Secret
    Repeated Parameter Should Be Refused    type=Vehicle&q=speed%3E1&q=speed%3C1
    # an empty first occurrence is still an occurrence: a guard counting only
    # the parameters it kept would never see this one collide
    Repeated Parameter Should Be Refused    type=&type=Secret
    # ...as is one written with no value at all
    Repeated Parameter Should Be Refused    type&type=Secret

A Percent Encoded Key Cannot Smuggle A Repeat
    [Documentation]    The repeat is counted on the DECODED key. %74 is "t", so
    ...    a guard comparing raw strings would pass the second occurrence
    ...    through and hand the policy layer in front exactly the disagreement
    ...    the guard exists to remove.
    [Tags]    antares-specific    query-params    parameter-pollution
    Repeated Parameter Should Be Refused    type=Vehicle&%74ype=Secret
    Repeated Parameter Should Be Refused    %74ype=Vehicle&type=Secret
    # "+" and "%20" both decode to a space, so both of these name the key "a b"
    Repeated Parameter Should Be Refused    a+b=1&a%20b=2

Conformant Queries Are Left Alone
    [Documentation]    The guard must refuse the ambiguity without refusing the
    ...    spec's own syntax. Entity Type Selection (4.17) puts a disjunction
    ...    inside ONE parameter, so a conformant client never repeats one.
    [Tags]    antares-specific    query-params
    Query Should Be Accepted    type=Vehicle,Building
    Query Should Be Accepted    type=Vehicle%7CBuilding
    Query Should Be Accepted    type=Vehicle&attrs=speed
    # 6.3.20 allows a known parameter to appear once carrying an empty value
    Query Should Be Accepted    type=Vehicle&attrs=


*** Keywords ***
Repeated Parameter Should Be Refused
    [Arguments]    ${query}
    ${response}=    GET    url=${url}/entities?${query}    expected_status=any
    Check Response Status Code    400    ${response.status_code}
    Should Be Equal As Strings    ${response.json()}[type]
    ...    https://uri.etsi.org/ngsi-ld/errors/InvalidRequest

Query Should Be Accepted
    [Arguments]    ${query}
    ${response}=    GET    url=${url}/entities?${query}    expected_status=any
    Check Response Status Code    200    ${response.status_code}
